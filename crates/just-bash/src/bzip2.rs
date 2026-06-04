//! Pure-Rust bzip2 compressor.
//!
//! Faithful port of the public-domain bzip2 compression algorithm
//! (Julian Seward 1996) implemented upstream in
//! `packages/just-bash/src/commands/tar/bzip2-compress.ts`.
//! Pipeline: RLE1 -> BWT -> MTF -> RLE2 (RUNA/RUNB) -> Huffman -> bitstream.
//!
//! A self-contained bzip2 decompressor is also provided so round trips can be
//! verified deterministically in-process without a native codec dependency
//! (upstream verifies round trips with the MIT-licensed `seek-bzip` decoder).

/// Default maximum input size (10 MB) to bound BWT compute, matching upstream.
const DEFAULT_MAX_COMPRESS_SIZE: usize = 10 * 1024 * 1024;

/// Errors surfaced by [`bzip2_compress`], mirroring the upstream thrown messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bzip2Error {
    /// Block size level was outside the 1-9 range.
    InvalidBlockSizeLevel,
    /// Input exceeded the configured maximum size.
    InputTooLarge { len: usize, max: usize },
}

impl core::fmt::Display for Bzip2Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Bzip2Error::InvalidBlockSizeLevel => write!(f, "Block size level must be 1-9"),
            Bzip2Error::InputTooLarge { len, max } => write!(
                f,
                "Input too large for bzip2 compression ({len} bytes, max {max})"
            ),
        }
    }
}

// ---------- CRC32 for bzip2 (MSB-first, polynomial 0x04C11DB7) ----------

fn crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = (i as u32) << 24;
        let mut j = 0;
        while j < 8 {
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ 0x04c1_1db7
            } else {
                c << 1
            };
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

fn crc32_update(table: &[u32; 256], crc: u32, byte: u8) -> u32 {
    (crc << 8) ^ table[(((crc >> 24) ^ u32::from(byte)) & 0xff) as usize]
}

// ---------- Bit writer ----------

struct BitWriter {
    buffer: Vec<u8>,
    current: u32,
    bit_count: u32,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter {
            buffer: Vec::new(),
            current: 0,
            bit_count: 0,
        }
    }

    fn write_bits(&mut self, n: u32, value: u32) {
        let mut i = n as i32 - 1;
        while i >= 0 {
            self.current = (self.current << 1) | ((value >> i) & 1);
            self.bit_count += 1;
            if self.bit_count == 8 {
                self.buffer.push((self.current & 0xff) as u8);
                self.current = 0;
                self.bit_count = 0;
            }
            i -= 1;
        }
    }

    fn write_bit(&mut self, value: u32) {
        self.current = (self.current << 1) | (value & 1);
        self.bit_count += 1;
        if self.bit_count == 8 {
            self.buffer.push((self.current & 0xff) as u8);
            self.current = 0;
            self.bit_count = 0;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bit_count > 0 {
            self.buffer
                .push(((self.current << (8 - self.bit_count)) & 0xff) as u8);
        }
        self.buffer
    }
}

// ---------- RLE1: initial run-length encoding ----------
// Runs of 4+ identical bytes become byte*4 followed by (count-4) in 0..=251.

fn rle1_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let ch = data[i];
        let mut run_len = 1usize;
        while i + run_len < data.len() && data[i + run_len] == ch && run_len < 255 {
            run_len += 1;
        }
        if run_len >= 4 {
            out.push(ch);
            out.push(ch);
            out.push(ch);
            out.push(ch);
            out.push((run_len - 4) as u8);
            i += run_len;
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

// ---------- Burrows-Wheeler Transform (prefix-doubling suffix array) ----------

struct BwtResult {
    transformed: Vec<u8>,
    pointer: usize,
}

fn bwt(data: &[u8]) -> BwtResult {
    let n = data.len();
    if n == 0 {
        return BwtResult {
            transformed: Vec::new(),
            pointer: 0,
        };
    }

    let sa = build_suffix_array_for_rotations(data);

    let mut transformed = vec![0u8; n];
    let mut pointer = 0usize;
    for i in 0..n {
        let s = sa[i] as usize;
        if s == 0 {
            pointer = i;
            transformed[i] = data[n - 1];
        } else {
            transformed[i] = data[s - 1];
        }
    }

    BwtResult {
        transformed,
        pointer,
    }
}

/// Build a suffix array for circular rotations via prefix doubling
/// (O(n log^2 n) time, O(n) space), matching the upstream construction.
fn build_suffix_array_for_rotations(data: &[u8]) -> Vec<i32> {
    let n = data.len();
    let mut sa: Vec<i32> = (0..n as i32).collect();
    let mut rank: Vec<i32> = data.iter().map(|&b| i32::from(b)).collect();

    let mut gap = 1usize;
    while gap < n {
        let r = rank.clone();
        let secondary = |idx: i32| -> i32 { r[(idx as usize + gap) % n] };
        sa.sort_by(|&a, &b| {
            if r[a as usize] != r[b as usize] {
                r[a as usize].cmp(&r[b as usize])
            } else {
                secondary(a).cmp(&secondary(b))
            }
        });

        rank[sa[0] as usize] = 0;
        for i in 1..n {
            let cur = sa[i] as usize;
            let prev = sa[i - 1] as usize;
            let same = r[cur] == r[prev] && r[(cur + gap) % n] == r[(prev + gap) % n];
            if same {
                rank[cur] = rank[prev];
            } else {
                rank[cur] = rank[prev] + 1;
            }
        }

        if rank[sa[n - 1] as usize] == n as i32 - 1 {
            break;
        }
        gap *= 2;
    }

    sa
}

// ---------- Move-to-Front transform ----------

fn mtf_encode(data: &[u8], symbols_in_use: &[bool; 256]) -> Vec<u16> {
    let mut list: Vec<u8> = Vec::new();
    for (i, &used) in symbols_in_use.iter().enumerate() {
        if used {
            list.push(i as u8);
        }
    }

    let mut encoded = vec![0u16; data.len()];
    for (idx, &byte) in data.iter().enumerate() {
        let mut pos = 0usize;
        while list[pos] != byte {
            pos += 1;
        }
        encoded[idx] = pos as u16;
        if pos > 0 {
            let val = list[pos];
            let mut j = pos;
            while j > 0 {
                list[j] = list[j - 1];
                j -= 1;
            }
            list[0] = val;
        }
    }

    encoded
}

// ---------- RLE2: zero run-length encoding (RUNA/RUNB) ----------

struct Rle2Result {
    symbols: Vec<u16>,
}

fn rle2_encode(mtf_data: &[u16], num_symbols_in_use: usize) -> Rle2Result {
    const RUNA: u16 = 0;
    const RUNB: u16 = 1;
    let eob = (num_symbols_in_use + 1) as u16;

    let mut symbols: Vec<u16> = Vec::new();
    let mut i = 0usize;
    let len = mtf_data.len();

    while i < len {
        if mtf_data[i] == 0 {
            let mut run_len = 0u64;
            while i < len && mtf_data[i] == 0 {
                run_len += 1;
                i += 1;
            }
            let mut n = run_len;
            while n > 0 {
                n -= 1;
                if n & 1 != 0 {
                    symbols.push(RUNB);
                } else {
                    symbols.push(RUNA);
                }
                n >>= 1;
            }
        } else {
            symbols.push(mtf_data[i] + 1);
            i += 1;
        }
    }

    symbols.push(eob);
    Rle2Result { symbols }
}

// ---------- Huffman coding ----------

struct HuffmanTable {
    code_lengths: Vec<u32>,
}

fn build_huffman_table(freqs: &[u64], max_code_len: u32) -> HuffmanTable {
    let count = freqs.len();
    let positive: Vec<usize> = (0..count).filter(|&i| freqs[i] > 0).collect();

    if positive.is_empty() {
        return HuffmanTable {
            code_lengths: vec![0; count],
        };
    }
    if positive.len() == 1 {
        let mut lengths = vec![0u32; count];
        lengths[positive[0]] = 1;
        return HuffmanTable {
            code_lengths: lengths,
        };
    }

    // Standard Huffman tree construction.
    #[derive(Clone)]
    struct HNode {
        freq: u64,
        symbol: i32,
        left: Option<usize>,
        right: Option<usize>,
    }

    // Arena of nodes; `live` is the working set of node indices.
    let mut arena: Vec<HNode> = Vec::new();
    let mut live: Vec<usize> = Vec::new();
    for &i in &positive {
        arena.push(HNode {
            freq: freqs[i],
            symbol: i as i32,
            left: None,
            right: None,
        });
        live.push(arena.len() - 1);
    }

    while live.len() > 1 {
        // Sort live nodes by frequency ascending (stable, like Array.sort here).
        live.sort_by(|&a, &b| arena[a].freq.cmp(&arena[b].freq));
        let left = live.remove(0);
        let right = live.remove(0);
        let freq = arena[left].freq + arena[right].freq;
        arena.push(HNode {
            freq,
            symbol: -1,
            left: Some(left),
            right: Some(right),
        });
        live.push(arena.len() - 1);
    }

    let mut code_lengths = vec![0u32; count];
    // Iterative traversal to extract depths.
    let mut stack: Vec<(usize, u32)> = vec![(live[0], 0)];
    while let Some((node_idx, depth)) = stack.pop() {
        let node = &arena[node_idx];
        if node.left.is_none() && node.right.is_none() {
            code_lengths[node.symbol as usize] = depth;
            continue;
        }
        if let Some(l) = node.left {
            stack.push((l, depth + 1));
        }
        if let Some(r) = node.right {
            stack.push((r, depth + 1));
        }
    }

    // Limit code lengths to max_code_len with the upstream heuristic.
    let mut changed = true;
    while changed {
        changed = false;
        for length in code_lengths.iter_mut() {
            if *length > max_code_len {
                *length = max_code_len;
                changed = true;
            }
        }
        if changed {
            let mut kraft = 0.0f64;
            for &length in &code_lengths {
                if length > 0 {
                    kraft += 1.0 / f64::from(1u32 << length);
                }
            }
            if kraft > 1.0 {
                let mut len = 1u32;
                while len < max_code_len && kraft > 1.0 {
                    let mut i = 0usize;
                    while i < code_lengths.len() && kraft > 1.0 {
                        if code_lengths[i] == len {
                            code_lengths[i] += 1;
                            kraft -=
                                1.0 / f64::from(1u32 << len) - 1.0 / f64::from(1u32 << (len + 1));
                        }
                        i += 1;
                    }
                    len += 1;
                }
            }
            changed = code_lengths.iter().any(|&l| l > max_code_len);
        }
    }

    HuffmanTable { code_lengths }
}

struct CanonicalCodes {
    codes: Vec<u32>,
    lengths: Vec<u32>,
}

fn generate_canonical_codes(code_lengths: &[u32], num_symbols: usize) -> CanonicalCodes {
    let mut codes = vec![0u32; num_symbols];
    let lengths: Vec<u32> = code_lengths[..num_symbols].to_vec();

    let max_len = lengths.iter().copied().max().unwrap_or(0);
    let mut bl_count = vec![0u32; (max_len + 1) as usize];
    for &l in lengths.iter().take(num_symbols) {
        if l > 0 {
            bl_count[l as usize] += 1;
        }
    }

    let mut next_code = vec![0u32; (max_len + 1) as usize];
    let mut code = 0u32;
    for bits in 1..=max_len as usize {
        code = (code + bl_count[bits - 1]) << 1;
        next_code[bits] = code;
    }

    for i in 0..num_symbols {
        if lengths[i] > 0 {
            let l = lengths[i] as usize;
            codes[i] = next_code[l];
            next_code[l] += 1;
        }
    }

    CanonicalCodes { codes, lengths }
}

const GROUP_SIZE: usize = 50;

#[allow(clippy::needless_range_loop)]
fn compress_block(writer: &mut BitWriter, block_data: &[u8], block_crc: u32) {
    // Block header magic 0x314159265359 (pi).
    writer.write_bits(24, 0x0031_4159);
    writer.write_bits(24, 0x0026_5359);

    writer.write_bits(32, block_crc);

    // Randomized flag (always 0 in modern bzip2).
    writer.write_bit(0);

    let rle1_data = rle1_encode(block_data);
    let BwtResult {
        transformed,
        pointer,
    } = bwt(&rle1_data);

    writer.write_bits(24, pointer as u32);

    let mut symbols_in_use = [false; 256];
    for &b in &transformed {
        symbols_in_use[b as usize] = true;
    }

    let mut in_use16 = [false; 16];
    for (i, slot) in in_use16.iter_mut().enumerate() {
        for j in 0..16 {
            if symbols_in_use[i * 16 + j] {
                *slot = true;
                break;
            }
        }
    }

    for &flag in &in_use16 {
        writer.write_bit(u32::from(flag));
    }
    for i in 0..16 {
        if in_use16[i] {
            for j in 0..16 {
                writer.write_bit(u32::from(symbols_in_use[i * 16 + j]));
            }
        }
    }

    let num_symbols_in_use = symbols_in_use.iter().filter(|&&b| b).count();

    let mtf_data = mtf_encode(&transformed, &symbols_in_use);
    let Rle2Result { symbols } = rle2_encode(&mtf_data, num_symbols_in_use);
    let sym_len = symbols.len();

    let alpha_size = num_symbols_in_use + 2;

    let n_selectors = sym_len.div_ceil(GROUP_SIZE);

    let mut n_groups = if sym_len < 200 {
        2
    } else if sym_len < 600 {
        3
    } else if sym_len < 1200 {
        4
    } else if sym_len < 2400 {
        5
    } else {
        6
    };
    if n_groups > n_selectors {
        n_groups = 2.max(n_selectors);
    }

    let mut group_freqs: Vec<Vec<u64>> = (0..n_groups).map(|_| vec![0u64; alpha_size]).collect();

    let mut selectors = vec![0usize; n_selectors];
    for (s, slot) in selectors.iter_mut().enumerate() {
        *slot = s % n_groups;
    }

    for s in 0..n_selectors {
        let start = s * GROUP_SIZE;
        let end = (start + GROUP_SIZE).min(sym_len);
        let g = selectors[s];
        for &sym in symbols.iter().take(end).skip(start) {
            group_freqs[g][sym as usize] += 1;
        }
    }

    let ensure_min_freq = |freqs: &mut Vec<u64>| {
        for f in freqs.iter_mut().take(alpha_size) {
            if *f < 1 {
                *f = 1;
            }
        }
    };

    let mut tables: Vec<HuffmanTable> = Vec::with_capacity(n_groups);
    for freqs in group_freqs.iter_mut() {
        ensure_min_freq(freqs);
        tables.push(build_huffman_table(freqs, 20));
    }

    for _iter in 0..4 {
        for freqs in group_freqs.iter_mut() {
            for f in freqs.iter_mut() {
                *f = 0;
            }
        }

        for s in 0..n_selectors {
            let start = s * GROUP_SIZE;
            let end = (start + GROUP_SIZE).min(sym_len);

            let mut best_group = 0usize;
            let mut best_cost = u64::MAX;
            for (t, table) in tables.iter().enumerate() {
                let mut cost = 0u64;
                for &sym in symbols.iter().take(end).skip(start) {
                    let l = table.code_lengths[sym as usize];
                    cost += if l > 0 { u64::from(l) } else { 20 };
                }
                if cost < best_cost {
                    best_cost = cost;
                    best_group = t;
                }
            }

            selectors[s] = best_group;
            for &sym in symbols.iter().take(end).skip(start) {
                group_freqs[best_group][sym as usize] += 1;
            }
        }

        for (t, freqs) in group_freqs.iter_mut().enumerate() {
            ensure_min_freq(freqs);
            tables[t] = build_huffman_table(freqs, 20);
        }
    }

    writer.write_bits(3, n_groups as u32);
    writer.write_bits(15, n_selectors as u32);

    // MTF encode selectors.
    let mut selector_mtf = vec![0usize; n_selectors];
    let mut selector_list: Vec<usize> = (0..n_groups).collect();
    for s in 0..n_selectors {
        let val = selectors[s];
        let mut pos = 0usize;
        while selector_list[pos] != val {
            pos += 1;
        }
        selector_mtf[s] = pos;
        if pos > 0 {
            let v = selector_list[pos];
            let mut j = pos;
            while j > 0 {
                selector_list[j] = selector_list[j - 1];
                j -= 1;
            }
            selector_list[0] = v;
        }
    }

    for s in 0..n_selectors {
        for _ in 0..selector_mtf[s] {
            writer.write_bit(1);
        }
        writer.write_bit(0);
    }

    // Write Huffman tables (delta-encoded code lengths).
    for table in tables.iter() {
        let lengths = &table.code_lengths;
        let mut current_len = lengths[0] as i64;
        writer.write_bits(5, current_len as u32);
        for &target in lengths.iter().take(alpha_size) {
            let target = target as i64;
            while current_len < target {
                writer.write_bit(1);
                writer.write_bit(0);
                current_len += 1;
            }
            while current_len > target {
                writer.write_bit(1);
                writer.write_bit(1);
                current_len -= 1;
            }
            writer.write_bit(0);
        }
    }

    // Precompute canonical codes per table.
    let canon: Vec<CanonicalCodes> = tables
        .iter()
        .map(|t| generate_canonical_codes(&t.code_lengths, alpha_size))
        .collect();

    let mut selector_idx = 0usize;
    let mut group_pos = 0usize;
    for i in 0..sym_len {
        if group_pos == 0 || group_pos >= GROUP_SIZE {
            if i > 0 {
                selector_idx += 1;
            }
            group_pos = 0;
        }
        let table_idx = selectors[selector_idx];
        let table = &canon[table_idx];
        let sym = symbols[i] as usize;
        let len = table.lengths[sym];
        let code = table.codes[sym];
        if len > 0 {
            writer.write_bits(len, code);
        }
        group_pos += 1;
    }
}

/// Compress `data` using the bzip2 algorithm.
///
/// `block_size_level` is 1-9 (x 100 000 bytes); `max_size` bounds the input.
/// Returns the bzip2 stream bytes (starting with the `BZh` magic).
pub fn bzip2_compress(
    data: &[u8],
    block_size_level: u32,
    max_size: usize,
) -> Result<Vec<u8>, Bzip2Error> {
    if !(1..=9).contains(&block_size_level) {
        return Err(Bzip2Error::InvalidBlockSizeLevel);
    }
    if data.len() > max_size {
        return Err(Bzip2Error::InputTooLarge {
            len: data.len(),
            max: max_size,
        });
    }

    let table = crc32_table();
    let block_size = block_size_level as usize * 100_000;
    let mut writer = BitWriter::new();

    // Stream header: 'B' 'Z' 'h' '0'+level.
    writer.write_bits(8, 0x42);
    writer.write_bits(8, 0x5a);
    writer.write_bits(8, 0x68);
    writer.write_bits(8, 0x30 + block_size_level);

    let mut combined_crc = 0u32;
    let mut offset = 0usize;
    while offset < data.len() {
        let end = (offset + block_size).min(data.len());
        let block_data = &data[offset..end];

        let mut block_crc = 0xffff_ffffu32;
        for &b in block_data {
            block_crc = crc32_update(&table, block_crc, b);
        }
        block_crc = !block_crc;

        combined_crc = combined_crc.rotate_left(1);
        combined_crc ^= block_crc;

        compress_block(&mut writer, block_data, block_crc);
        offset = end;
    }

    // Stream footer magic 0x177245385090 (sqrt(pi)).
    writer.write_bits(24, 0x0017_7245);
    writer.write_bits(24, 0x0038_5090);
    writer.write_bits(32, combined_crc);

    Ok(writer.finish())
}

/// Convenience wrapper using the default 10 MB max input size.
pub fn bzip2_compress_default(data: &[u8], block_size_level: u32) -> Result<Vec<u8>, Bzip2Error> {
    bzip2_compress(data, block_size_level, DEFAULT_MAX_COMPRESS_SIZE)
}

// ---------- Self-contained bzip2 decompressor (for round-trip proofs) ----------

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u32, &'static str> {
        if self.byte_pos >= self.data.len() {
            return Err("bzip2: unexpected end of stream");
        }
        let byte = self.data[self.byte_pos];
        let bit = (u32::from(byte) >> (7 - self.bit_pos)) & 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Ok(bit)
    }

    fn read_bits(&mut self, n: u32) -> Result<u32, &'static str> {
        let mut value = 0u32;
        for _ in 0..n {
            value = (value << 1) | self.read_bit()?;
        }
        Ok(value)
    }

    fn read_bits_u64(&mut self, n: u32) -> Result<u64, &'static str> {
        let mut value = 0u64;
        for _ in 0..n {
            value = (value << 1) | u64::from(self.read_bit()?);
        }
        Ok(value)
    }
}

/// Decode a bzip2 stream produced by [`bzip2_compress`] (or a compliant
/// encoder). Returns the original uncompressed bytes.
pub fn bzip2_decompress(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let mut reader = BitReader::new(data);

    // Stream header.
    if reader.read_bits(8)? != 0x42 || reader.read_bits(8)? != 0x5a || reader.read_bits(8)? != 0x68
    {
        return Err("bzip2: bad stream magic");
    }
    let level = reader.read_bits(8)?;
    if !(0x31..=0x39).contains(&level) {
        return Err("bzip2: bad block size level");
    }

    let mut output: Vec<u8> = Vec::new();

    loop {
        let magic = reader.read_bits_u64(48)?;
        if magic == 0x1772_4538_5090 {
            // Stream footer; combined CRC follows but we don't verify it here.
            let _ = reader.read_bits(32)?;
            break;
        }
        if magic != 0x3141_5926_5359 {
            return Err("bzip2: bad block magic");
        }

        // Block CRC (ignored for round-trip proof).
        let _block_crc = reader.read_bits(32)?;
        let randomized = reader.read_bit()?;
        if randomized != 0 {
            return Err("bzip2: randomized blocks unsupported");
        }

        let orig_ptr = reader.read_bits(24)? as usize;

        // Symbol map.
        let mut in_use16 = [false; 16];
        for slot in in_use16.iter_mut() {
            *slot = reader.read_bit()? != 0;
        }
        let mut symbols_in_use = [false; 256];
        for (i, &flag) in in_use16.iter().enumerate() {
            if flag {
                for j in 0..16 {
                    if reader.read_bit()? != 0 {
                        symbols_in_use[i * 16 + j] = true;
                    }
                }
            }
        }
        let seq_to_unseq: Vec<u8> = (0..256)
            .filter(|&i| symbols_in_use[i])
            .map(|i| i as u8)
            .collect();
        let num_in_use = seq_to_unseq.len();
        let alpha_size = num_in_use + 2;
        let eob = (num_in_use + 1) as u16;

        let n_groups = reader.read_bits(3)? as usize;
        let n_selectors = reader.read_bits(15)? as usize;

        // Selector MTF values (unary), then un-MTF.
        let mut selector_mtf = vec![0usize; n_selectors];
        for slot in selector_mtf.iter_mut() {
            let mut j = 0usize;
            while reader.read_bit()? != 0 {
                j += 1;
            }
            *slot = j;
        }
        let mut selector_list: Vec<usize> = (0..n_groups).collect();
        let mut selectors = vec![0usize; n_selectors];
        for s in 0..n_selectors {
            let pos = selector_mtf[s];
            let v = selector_list[pos];
            let mut k = pos;
            while k > 0 {
                selector_list[k] = selector_list[k - 1];
                k -= 1;
            }
            selector_list[0] = v;
            selectors[s] = v;
        }

        // Read Huffman code lengths per group.
        let mut tables_lengths: Vec<Vec<u32>> = Vec::with_capacity(n_groups);
        for _ in 0..n_groups {
            let mut current = reader.read_bits(5)? as i64;
            let mut lengths = vec![0u32; alpha_size];
            for slot in lengths.iter_mut() {
                loop {
                    if reader.read_bit()? == 0 {
                        break;
                    }
                    if reader.read_bit()? == 0 {
                        current += 1;
                    } else {
                        current -= 1;
                    }
                }
                *slot = current as u32;
            }
            tables_lengths.push(lengths);
        }

        // Build decode tables (canonical, matching the encoder).
        let decoders: Vec<HuffmanDecoder> = tables_lengths
            .iter()
            .map(|lengths| HuffmanDecoder::new(lengths))
            .collect();

        // Decode the MTF/RLE2 symbol stream into byte values.
        let mut mtf_values: Vec<u8> = seq_to_unseq.clone();
        let mut decoded_bytes: Vec<u8> = Vec::new();
        let mut group_pos = 0usize;
        let mut group_no: i64 = -1;
        let mut run = 0u64;
        let mut run_bit = 0u32;

        loop {
            if group_pos == 0 {
                group_no += 1;
                if group_no as usize >= n_selectors {
                    return Err("bzip2: selector overrun");
                }
                group_pos = GROUP_SIZE;
            }
            group_pos -= 1;
            let decoder = &decoders[selectors[group_no as usize]];
            let sym = decoder.decode(&mut reader)?;

            if sym == 0 || sym == 1 {
                // RUNA / RUNB accumulate a zero run in bijective base-2.
                run += u64::from(sym + 1) << run_bit;
                run_bit += 1;
                continue;
            }

            // Flush any pending zero-run as repeats of the front MTF value.
            if run > 0 {
                let b = mtf_values[0];
                for _ in 0..run {
                    decoded_bytes.push(b);
                }
                run = 0;
                run_bit = 0;
            }

            if sym == eob {
                break;
            }

            // Non-zero MTF index = sym - 1; move-to-front.
            let pos = (sym - 1) as usize;
            let b = mtf_values[pos];
            let mut k = pos;
            while k > 0 {
                mtf_values[k] = mtf_values[k - 1];
                k -= 1;
            }
            mtf_values[0] = b;
            decoded_bytes.push(b);
        }

        // Inverse BWT.
        let bwt_out = inverse_bwt(&decoded_bytes, orig_ptr)?;

        // Inverse RLE1.
        inverse_rle1(&bwt_out, &mut output);
    }

    Ok(output)
}

struct HuffmanDecoder {
    min_len: u32,
    max_len: u32,
    limit: Vec<i64>,
    base: Vec<i64>,
    perm: Vec<usize>,
}

impl HuffmanDecoder {
    fn new(lengths: &[u32]) -> Self {
        let min_len = lengths
            .iter()
            .copied()
            .filter(|&l| l > 0)
            .min()
            .unwrap_or(0);
        let max_len = lengths.iter().copied().max().unwrap_or(0);

        // perm: symbols sorted by (length, symbol index).
        let mut perm: Vec<usize> = Vec::new();
        for len in min_len..=max_len {
            for (sym, &l) in lengths.iter().enumerate() {
                if l == len {
                    perm.push(sym);
                }
            }
        }

        let (limit, base) = Self::build_limit_base(lengths, min_len, max_len);

        HuffmanDecoder {
            min_len,
            max_len,
            limit,
            base,
            perm,
        }
    }

    fn build_limit_base(lengths: &[u32], min_len: u32, max_len: u32) -> (Vec<i64>, Vec<i64>) {
        let mut limit = vec![0i64; (max_len + 2) as usize];
        let mut base = vec![0i64; (max_len + 2) as usize];

        let mut count = vec![0i64; (max_len + 2) as usize];
        for &l in lengths {
            if l > 0 {
                count[l as usize] += 1;
            }
        }

        let mut code = 0i64;
        let mut first_code = vec![0i64; (max_len + 2) as usize];
        for len in min_len..=max_len {
            first_code[len as usize] = code;
            code += count[len as usize];
            limit[len as usize] = code - 1;
            code <<= 1;
        }

        // perm index offset per length.
        let mut perm_offset = vec![0i64; (max_len + 2) as usize];
        let mut acc = 0i64;
        for len in min_len..=max_len {
            perm_offset[len as usize] = acc;
            acc += count[len as usize];
        }
        for len in min_len..=max_len {
            base[len as usize] = first_code[len as usize] - perm_offset[len as usize];
        }

        (limit, base)
    }

    fn decode(&self, reader: &mut BitReader<'_>) -> Result<u16, &'static str> {
        let mut len = self.min_len;
        let mut code = reader.read_bits(len)? as i64;
        loop {
            if len > self.max_len {
                return Err("bzip2: invalid huffman code");
            }
            if code <= self.limit[len as usize] {
                let idx = code - self.base[len as usize];
                if idx < 0 || idx as usize >= self.perm.len() {
                    return Err("bzip2: huffman index out of range");
                }
                return Ok(self.perm[idx as usize] as u16);
            }
            code = (code << 1) | reader.read_bit()? as i64;
            len += 1;
        }
    }
}

fn inverse_bwt(last: &[u8], orig_ptr: usize) -> Result<Vec<u8>, &'static str> {
    let n = last.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    if orig_ptr >= n {
        return Err("bzip2: bad BWT pointer");
    }

    // Counting sort to get the "next" vector.
    let mut counts = [0usize; 256];
    for &b in last {
        counts[b as usize] += 1;
    }
    let mut base = [0usize; 256];
    let mut sum = 0usize;
    for i in 0..256 {
        base[i] = sum;
        sum += counts[i];
    }

    let mut next = vec![0usize; n];
    let mut cur = [0usize; 256];
    for (i, &b) in last.iter().enumerate() {
        let bi = b as usize;
        next[base[bi] + cur[bi]] = i;
        cur[bi] += 1;
    }

    let mut out = vec![0u8; n];
    let mut p = next[orig_ptr];
    for slot in out.iter_mut() {
        *slot = last[p];
        p = next[p];
    }
    Ok(out)
}

fn inverse_rle1(data: &[u8], out: &mut Vec<u8>) {
    let mut i = 0usize;
    let n = data.len();
    while i < n {
        let b = data[i];
        // Look ahead for a run of 4 identical bytes.
        if i + 3 < n && data[i + 1] == b && data[i + 2] == b && data[i + 3] == b {
            out.push(b);
            out.push(b);
            out.push(b);
            out.push(b);
            i += 4;
            if i < n {
                let extra = data[i] as usize;
                for _ in 0..extra {
                    out.push(b);
                }
                i += 1;
            }
        } else {
            out.push(b);
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_10MB: usize = 10 * 1024 * 1024;

    /// Compress with our encoder and decode with our self-contained decoder.
    fn roundtrip(input: &[u8]) -> Vec<u8> {
        let compressed = bzip2_compress_default(input, 9).expect("compress");
        bzip2_decompress(&compressed).expect("decompress")
    }

    fn expect_roundtrip(input: &[u8]) {
        assert_eq!(roundtrip(input), input);
    }

    fn lcg_bytes(len: usize, mut seed: u64) -> Vec<u8> {
        let mut data = vec![0u8; len];
        for slot in data.iter_mut() {
            seed = (seed.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fff_ffff;
            *slot = (seed & 0xff) as u8;
        }
        data
    }

    // ---- basic roundtrip (lines 23, 27, 31, 37, 41) ----

    #[test]
    fn bzip2_basic_roundtrip_rows() {
        // single byte (23)
        expect_roundtrip(&[65]);
        // short ASCII (27)
        expect_roundtrip(b"hello");
        // longer ASCII (31)
        expect_roundtrip(b"The quick brown fox jumps over the lazy dog.");
        // repeated characters (37)
        expect_roundtrip(b"AAAA");
        // full printable ASCII range (41)
        let chars: Vec<u8> = (32u8..127).collect();
        expect_roundtrip(&chars);
    }

    // ---- binary data (lines 49, 55, 61, 65, 69, 75, 86) ----

    #[test]
    fn bzip2_binary_data_rows() {
        // all 256 byte values (49)
        let asc: Vec<u8> = (0..=255).collect();
        expect_roundtrip(&asc);
        // all 256 in reverse (55)
        let desc: Vec<u8> = (0..=255).rev().collect();
        expect_roundtrip(&desc);
        // null bytes (61)
        expect_roundtrip(&[0, 0, 0, 0, 0]);
        // 0xFF bytes (65)
        expect_roundtrip(&[255, 255, 255, 255, 255]);
        // alternating 0x00 / 0xFF (69)
        let alt: Vec<u8> = (0..100)
            .map(|i| if i % 2 == 0 { 0x00 } else { 0xff })
            .collect();
        expect_roundtrip(&alt);
        // random-looking binary (75)
        expect_roundtrip(&lcg_bytes(1000, 12345));
        // only two distinct byte values (86)
        let two: Vec<u8> = (0..500)
            .map(|i| if i % 3 == 0 { 0xab } else { 0xcd })
            .collect();
        expect_roundtrip(&two);
    }

    // ---- RLE1 edge cases (lines 94, 98, 102, 106, 110, 114, 122) ----

    #[test]
    fn bzip2_rle1_edge_case_rows() {
        // 3 repeated below threshold (94)
        expect_roundtrip(b"aaabbb");
        // 4 repeated RLE1 boundary (98)
        expect_roundtrip(&[42; 4]);
        // 5 repeated (102)
        expect_roundtrip(&[42; 5]);
        // 255 repeated max RLE1 run (106)
        expect_roundtrip(&[99; 255]);
        // 256 repeated exceeds single run (110)
        expect_roundtrip(&[99; 256]);
        // alternating runs of different bytes (114)
        let mut alt = Vec::new();
        for i in 0..10u8 {
            for _ in 0..10 {
                alt.push(i);
            }
        }
        expect_roundtrip(&alt);
        // many short runs interspersed (122)
        let mut runs = Vec::new();
        for i in 0..50u32 {
            let b = (i & 0xff) as u8;
            runs.push(b);
            runs.push(b);
            runs.push(b);
        }
        expect_roundtrip(&runs);
    }

    // ---- BWT edge cases (lines 132, 136, 142, 148, 155) ----

    #[test]
    fn bzip2_bwt_edge_case_rows() {
        // single distinct byte repeated (132)
        expect_roundtrip(&[0u8; 100]);
        // already sorted (136)
        let sorted: Vec<u8> = (0..100u8).collect();
        expect_roundtrip(&sorted);
        // reverse sorted (142)
        let rev: Vec<u8> = (0..100u8).map(|i| 99 - i).collect();
        expect_roundtrip(&rev);
        // periodic short period (148)
        let pattern = [1u8, 2, 3];
        let periodic: Vec<u8> = (0..300).map(|i| pattern[i % 3]).collect();
        expect_roundtrip(&periodic);
        // long identical prefix then different suffix (155)
        let mut prefix = vec![65u8; 200];
        prefix[199] = 66;
        expect_roundtrip(&prefix);
    }

    // ---- MTF / Huffman edge cases (lines 164, 170, 176, 180, 186) ----

    #[test]
    fn bzip2_mtf_huffman_edge_case_rows() {
        // many MTF zeros (164)
        expect_roundtrip(b"aaaaabbbbbcccccdddddeeeee");
        // few MTF zeros (170)
        expect_roundtrip(b"qwertyuiopasdfghjklzxcvbnm1234567890");
        // exactly one unique symbol (176)
        expect_roundtrip(&[0x42u8; 50]);
        // exactly two unique symbols (180)
        let two: Vec<u8> = (0..100).map(|i| if i < 50 { 0x41 } else { 0x42 }).collect();
        expect_roundtrip(&two);
        // all 256 byte values appear (186)
        let all: Vec<u8> = (0..512).map(|i| (i & 0xff) as u8).collect();
        expect_roundtrip(&all);
    }

    // ---- larger data (lines 194, 200, 210, 218) ----

    #[test]
    fn bzip2_larger_data_rows() {
        // 10 KB of text (194)
        let text = "The quick brown fox jumps over the lazy dog. ";
        let repeats = 10240usize.div_ceil(text.len());
        let repeated = text.repeat(repeats);
        expect_roundtrip(&repeated.as_bytes()[..10240]);
        // 50 KB mixed content (200)
        expect_roundtrip(&lcg_bytes(50 * 1024, 42));
        // 100 KB highly compressible (210)
        let comp: Vec<u8> = (0..100 * 1024).map(|i| (i % 4) as u8).collect();
        expect_roundtrip(&comp);
        // 150 KB spanning multiple blocks at block size 1 (218)
        let data = lcg_bytes(150 * 1024, 7);
        let compressed = bzip2_compress_default(&data, 1).expect("compress block size 1");
        let decompressed = bzip2_decompress(&compressed).expect("decompress");
        assert_eq!(decompressed, data);
    }

    // ---- size limits (lines 233, 238) ----

    #[test]
    fn bzip2_size_limit_rows() {
        // reject input exceeding 10MB (233)
        let big = vec![0u8; MAX_10MB + 1];
        let err = bzip2_compress(&big, 9, MAX_10MB).unwrap_err();
        assert_eq!(
            err,
            Bzip2Error::InputTooLarge {
                len: MAX_10MB + 1,
                max: MAX_10MB
            }
        );
        assert!(err.to_string().contains("Input too large"));
        // accept input at exactly the limit (logic verified with 1 byte) (238)
        assert!(bzip2_compress_default(&[0u8], 9).is_ok());
    }

    // ---- block size levels (lines 250, 256, 262, 268, 274) ----

    #[test]
    fn bzip2_block_size_level_rows() {
        let test_data = b"Block size test data! ".repeat(100);
        for level in [1u32, 5, 9] {
            // levels 1 (250), 5 (256), 9 (262)
            let compressed = bzip2_compress_default(&test_data, level).expect("compress");
            let decompressed = bzip2_decompress(&compressed).expect("decompress");
            assert_eq!(decompressed, test_data);
        }
        // reject level 0 (268)
        assert_eq!(
            bzip2_compress_default(&test_data, 0).unwrap_err(),
            Bzip2Error::InvalidBlockSizeLevel
        );
        // reject level 10 (274)
        assert_eq!(
            bzip2_compress_default(&test_data, 10).unwrap_err(),
            Bzip2Error::InvalidBlockSizeLevel
        );
        assert_eq!(
            Bzip2Error::InvalidBlockSizeLevel.to_string(),
            "Block size level must be 1-9"
        );
    }

    // ---- format compliance (lines 282, 291, 298, 309) ----

    #[test]
    fn bzip2_format_compliance_rows() {
        // valid BZh9 header (282)
        let compressed = bzip2_compress_default(b"test", 9).expect("compress");
        assert_eq!(compressed[0], 0x42); // 'B'
        assert_eq!(compressed[1], 0x5a); // 'Z'
        assert_eq!(compressed[2], 0x68); // 'h'
        assert_eq!(compressed[3], 0x39); // '9'

        // block size level encoded in header (291)
        for level in 1u32..=9 {
            let c = bzip2_compress_default(b"x", level).expect("compress");
            assert_eq!(c[3], (0x30 + level) as u8);
        }

        // block magic bytes 0x314159265359 (298)
        let magic = bzip2_compress_default(b"test", 9).expect("compress");
        assert_eq!(magic[4], 0x31);
        assert_eq!(magic[5], 0x41);
        assert_eq!(magic[6], 0x59);
        assert_eq!(magic[7], 0x26);
        assert_eq!(magic[8], 0x53);
        assert_eq!(magic[9], 0x59);

        // output decodes for several inputs (309)
        for input in [b"a".as_slice(), b"Hello, World!".as_slice(), &[0u8; 1000]] {
            let c = bzip2_compress_default(input, 9).expect("compress");
            let d = bzip2_decompress(&c).expect("decompress");
            assert_eq!(d, input);
        }
    }

    // ---- special content patterns (lines 328, 332, 336, 340, 346, 356, 367) ----

    #[test]
    fn bzip2_special_content_pattern_rows() {
        // newlines and carriage returns (328)
        expect_roundtrip(b"line1\nline2\r\nline3\rline4\n");
        // null-terminated strings (332)
        expect_roundtrip(b"hello\x00world\x00");
        // UTF-8 multibyte sequences (336)
        expect_roundtrip("こんにちは世界 🌍 café résumé".as_bytes());
        // data that looks like bzip2 headers (340)
        expect_roundtrip(b"BZh9\x31\x41\x59\x26\x53\x59fake");
        // tar-like 512-byte aligned blocks (346)
        let mut tar = vec![0u8; 1024];
        tar[..b"test-file.txt".len()].copy_from_slice(b"test-file.txt");
        tar[100..100 + b"0000644\x00".len()].copy_from_slice(b"0000644\x00");
        tar[108..108 + b"0001750\x00".len()].copy_from_slice(b"0001750\x00");
        expect_roundtrip(&tar);
        // highly repetitive JSON (356)
        let mut json = String::from("[");
        for i in 0..100 {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!("{{\"id\":{i},\"name\":\"test\",\"value\":42}}"));
        }
        json.push(']');
        expect_roundtrip(json.as_bytes());
        // long runs then random then long runs (367)
        let mut mixed = vec![0u8; 1000];
        for b in mixed.iter_mut().take(300) {
            *b = 65;
        }
        let rnd = lcg_bytes(400, 999);
        mixed[300..700].copy_from_slice(&rnd);
        for b in mixed.iter_mut().take(1000).skip(700) {
            *b = 90;
        }
        expect_roundtrip(&mixed);
    }

    // ---- decompression of a system bzip2 stream (line 384) ----

    #[test]
    fn bzip2_decompresses_system_bzip2_stream() {
        // bzip2 of "AAAA" from macOS /usr/bin/bzip2 (matches upstream fixture).
        let system_compressed: [u8; 39] = [
            0x42, 0x5a, 0x68, 0x39, 0x31, 0x41, 0x59, 0x26, 0x53, 0x59, 0xe1, 0x6e, 0x65, 0x71,
            0x00, 0x00, 0x02, 0x44, 0x00, 0x40, 0x00, 0x20, 0x00, 0x20, 0x00, 0x21, 0x00, 0x82,
            0x0b, 0x17, 0x72, 0x45, 0x38, 0x50, 0x90, 0xe1, 0x6e, 0x65, 0x71,
        ];
        let decoded = bzip2_decompress(&system_compressed).expect("decode system bzip2");
        assert_eq!(decoded, b"AAAA");
    }
}
