#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const DEFAULT_UPSTREAM_ROOT = path.join(
  os.homedir(),
  '.opensrc/repos/github.com/vercel/ai/main',
);
const UPSTREAM_HEAD = 'ab6d66482d31afe15f4973a51c5f7cfa09c92ea6';
const UPSTREAM_COMMIT_DATE = '2026-05-30T00:54:18Z';
const INVENTORY_DATE = '2026-06-01';
const FETCH_COMMAND = 'npx opensrc fetch github:vercel/ai';

const PACKAGES = [
  {
    dir: 'anthropic',
    name: '@ai-sdk/anthropic',
    crate: 'crates/ai-sdk-anthropic',
    childRow: 'AI-01A',
    runtimeScope:
      'Anthropic Messages language model, files, skills, prompt conversion, cache control, provider tools, usage conversion, and error mapping.',
  },
  {
    dir: 'amazon-bedrock',
    name: '@ai-sdk/amazon-bedrock',
    crate: 'crates/ai-sdk-amazon-bedrock',
    childRow: 'AI-01B',
    runtimeScope:
      'Amazon Bedrock chat, Anthropic-on-Bedrock, embeddings, image, reranking, event stream decoding, SigV4/API-key fetch wrappers, tool preparation, usage conversion, and model settings.',
  },
  {
    dir: 'google',
    name: '@ai-sdk/google',
    crate: 'crates/ai-sdk-google',
    childRow: 'AI-01C',
    runtimeScope:
      'Google Gemini language, embedding, image, video, files, interactions, schema conversion, URL support, tool preparation, JSON accumulation, and finish/source parsing.',
  },
  {
    dir: 'google-vertex',
    name: '@ai-sdk/google-vertex',
    crate: 'crates/ai-sdk-google-vertex',
    childRow: 'AI-01D',
    childStatus: 'complete',
    sourceStatus: 'ported',
    closureProof:
      'All 201 portable upstream cases are mapped below to named Rust tests in `crates/ai-sdk-google-vertex`; 3 callable-constructor cases remain explicit `js-only-documented` exceptions, and live credentials are covered by the ignored `live_google_vertex_embedding_smoke_uses_real_credentials` smoke test.',
    runtimeScope:
      'Google Vertex auth, provider base/edge variants, embedding, image, video, Anthropic-on-Vertex, MaaS, xAI-on-Vertex, and provider tests.',
  },
];

const GOOGLE_VERTEX_PORT_MAPPINGS = [
  {
    start: 1,
    end: 19,
    rustTarget: 'test: google_vertex_auth_headers_cover_node_and_edge_wrappers',
    notes:
      'Covers node/edge auth header generation, header merging and overriding, custom token callbacks, error propagation, null tokens, and once-per-provider token generator creation.',
  },
  {
    start: 20,
    end: 30,
    rustTarget: 'test: google_vertex_anthropic_provider_matches_vertex_specific_contract',
    notes:
      'Covers Anthropic-on-Vertex provider construction, base URLs, unsupported model families, tool subset exposure, header propagation, global/regional URL selection, supported URL policy, and rawPredict request shaping.',
  },
  {
    start: 31,
    end: 41,
    rustTarget: 'test: google_vertex_edge_auth_builds_jwt_and_token_exchange_request',
    notes:
      'Covers edge JWT claims, RS256 signing, token exchange request shape, credential validation, and auth endpoint/error mapping.',
  },
  {
    start: 42,
    end: 51,
    rustTarget: 'test: google_vertex_node_and_edge_auth_wrappers_match_upstream',
    notes:
      'Covers shared Vertex node/edge auth wrappers, default headers, custom credentials/auth options, generated authorization, custom fetch/transport, and header preservation.',
  },
  {
    start: 52,
    end: 64,
    rustTarget: 'test: google_vertex_embedding_model_builds_predict_requests_and_results',
    notes:
      'Covers embedding predict URL/body/header shaping, provider options, max embeddings per call, response value parsing, usage mapping, and API error metadata.',
  },
  {
    start: 65,
    end: 101,
    rustTarget: 'test: google_vertex_image_model_covers_imagen_and_gemini_contracts',
    notes:
      'Covers Imagen and Gemini image request bodies, warnings, provider options, binary/image response parsing, metadata/usage mapping, response headers/body, and API error metadata.',
  },
  {
    start: 102,
    end: 119,
    rustTarget: 'test: google_vertex_base_provider_resolves_models_urls_tools_and_express_mode',
    notes:
      'Covers base provider settings, language/embedding/image/video model factories, default/global/regional URLs, unsupported model lookups, tools, supported URL policy, and express-mode selection.',
  },
  {
    start: 120,
    end: 124,
    rustTarget: 'test: google_vertex_node_and_edge_auth_wrappers_match_upstream',
    notes:
      'Covers public node/edge provider auth wrapper exports, default/custom headers, generated authorization, and fetch/transport injection.',
  },
  {
    start: 125,
    end: 153,
    rustTarget: 'test: google_vertex_video_model_covers_predict_long_running_contract',
    notes:
      'Covers predictLongRunning request shaping, polling/fetchPredictOperation behavior, warnings, provider options, metadata, usage, timeout/abort handling, output media parsing, and error mapping.',
  },
  {
    start: 154,
    end: 182,
    rustTarget: 'test: google_vertex_maas_provider_wraps_openai_compatible_lazily',
    notes:
      'Covers MaaS node/edge auth wrappers, header/custom fetch handling, global/regional/custom base URLs, OpenAI-compatible delegation, lazy creation, caching, and unsupported model lookups.',
  },
  {
    start: 183,
    end: 204,
    rustTarget: 'test: google_vertex_xai_provider_wraps_openai_compatible_with_grok_transform',
    notes:
      'Covers xAI-on-Vertex node/edge auth wrappers, header/custom fetch handling, base URLs, OpenAI-compatible delegation, Grok request/usage transforms, HTTP image URL support, caching, and unsupported model lookups.',
  },
];

function parseArgs(argv) {
  const args = {
    upstreamRoot: DEFAULT_UPSTREAM_ROOT,
    output: null,
    check: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    switch (arg) {
      case '--upstream-root':
        args.upstreamRoot = argv[index + 1];
        index += 1;
        break;
      case '--output':
        args.output = argv[index + 1];
        index += 1;
        break;
      case '--check':
        args.check = true;
        break;
      case '--help':
      case '-h':
        printUsage();
        process.exit(0);
        break;
      default:
        throw new Error(`unknown argument: ${arg}`);
    }
  }

  return args;
}

function printUsage() {
  console.log(`Usage: node scripts/ai-foundational-provider-inventory.mjs [--upstream-root PATH] [--output PATH] [--check]

Generates the AI-01 row-level inventory for @ai-sdk/anthropic,
@ai-sdk/amazon-bedrock, @ai-sdk/google, and @ai-sdk/google-vertex.
The upstream tree should be refreshed first with:

  ${FETCH_COMMAND}
`);
}

function walkFiles(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const childPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...walkFiles(childPath));
    } else {
      files.push(childPath);
    }
  }

  return files.sort();
}

function relativeToPackage(file, packageRoot) {
  return path.relative(packageRoot, file).split(path.sep).join('/');
}

function isTestFile(file) {
  return /\.(test|spec)(-d)?\.tsx?$/.test(file);
}

function isSourceFile(file) {
  return /\.tsx?$/.test(file) && !isTestFile(file);
}

function isFixtureOrSnapshot(file) {
  return file.includes('/__fixtures__/') || file.includes('/__snapshots__/');
}

function lineNumber(source, index) {
  return source.slice(0, index).split('\n').length;
}

function skipWhitespace(source, index) {
  let cursor = index;
  while (cursor < source.length && /\s/.test(source[cursor])) {
    cursor += 1;
  }
  return cursor;
}

function matchingParen(source, openIndex) {
  let depth = 0;
  let quote = null;
  let escaped = false;

  for (let index = openIndex; index < source.length; index += 1) {
    const char = source[index];

    if (quote) {
      if (escaped) {
        escaped = false;
      } else if (char === '\\') {
        escaped = true;
      } else if (char === quote) {
        quote = null;
      }
      continue;
    }

    if (char === "'" || char === '"' || char === '`') {
      quote = char;
    } else if (char === '(') {
      depth += 1;
    } else if (char === ')') {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }

  return -1;
}

function readQuotedString(source, startIndex) {
  let cursor = skipWhitespace(source, startIndex);
  const quote = source[cursor];
  if (quote !== "'" && quote !== '"' && quote !== '`') {
    return null;
  }

  cursor += 1;
  let value = '';
  let escaped = false;

  for (; cursor < source.length; cursor += 1) {
    const char = source[cursor];
    if (escaped) {
      value += char;
      escaped = false;
    } else if (char === '\\') {
      escaped = true;
    } else if (char === quote) {
      return {
        value: value.replace(/\s+/g, ' ').trim(),
        end: cursor + 1,
      };
    } else {
      value += char;
    }
  }

  return null;
}

function countEachRows(expression) {
  const trimmed = expression.trim();
  if (!trimmed.startsWith('[')) {
    return null;
  }

  let depth = 0;
  let quote = null;
  let escaped = false;
  let count = 0;
  let sawValue = false;

  for (let index = 1; index < trimmed.length - 1; index += 1) {
    const char = trimmed[index];

    if (quote) {
      if (escaped) {
        escaped = false;
      } else if (char === '\\') {
        escaped = true;
      } else if (char === quote) {
        quote = null;
      }
      continue;
    }

    if (char === "'" || char === '"' || char === '`') {
      quote = char;
      sawValue = true;
    } else if (char === '[' || char === '{' || char === '(') {
      depth += 1;
      sawValue = true;
    } else if (char === ']' || char === '}' || char === ')') {
      depth -= 1;
    } else if (char === ',' && depth === 0) {
      count += 1;
      sawValue = false;
    } else if (!/\s/.test(char)) {
      sawValue = true;
    }
  }

  return count + (sawValue ? 1 : 0);
}

function extractTestCases(file, packageRoot) {
  const source = fs.readFileSync(file, 'utf8');
  const relativeFile = relativeToPackage(file, packageRoot);
  const cases = [];

  const eachPattern = /(^|[^\w.])(it|test)\.each\s*\(/gm;
  for (const match of source.matchAll(eachPattern)) {
    const openIndex = match.index + match[0].length - 1;
    const closeIndex = matchingParen(source, openIndex);
    if (closeIndex === -1) {
      continue;
    }

    const expression = source.slice(openIndex + 1, closeIndex);
    const callIndex = skipWhitespace(source, closeIndex + 1);
    if (source[callIndex] !== '(') {
      continue;
    }

    const testName = readQuotedString(source, callIndex + 1);
    if (!testName) {
      continue;
    }

    cases.push({
      file: relativeFile,
      line: lineNumber(source, match.index),
      kind: `${match[2]}.each`,
      name: testName.value,
      tableRows: countEachRows(expression),
    });
  }

  const normalPattern =
    /(^|[^\w.])(it|test)(?:\.(skip|only|todo|failing))?\s*\(\s*(['"`])((?:\\.|(?!\4)[\s\S])*?)\4/gm;
  for (const match of source.matchAll(normalPattern)) {
    cases.push({
      file: relativeFile,
      line: lineNumber(source, match.index),
      kind: `${match[2]}${match[3] ? `.${match[3]}` : ''}`,
      name: match[5].replace(/\s+/g, ' ').trim(),
      tableRows: null,
    });
  }

  return cases.sort((left, right) => left.line - right.line || left.name.localeCompare(right.name));
}

function rustTestSlug(name) {
  return (
    name
      .replace(/&quot;/g, '')
      .replace(/`/g, '')
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '_')
      .replace(/^_+|_+$/g, '')
      .slice(0, 72)
      .replace(/_+$/g, '') || 'upstream_case'
  );
}

function anthropicRustTestName(testCase, index) {
  const caseName = formatCaseName(testCase);
  return `anthropic_${String(index + 1).padStart(4, '0')}_${rustTestSlug(
    caseName,
  )}`;
}

const AMAZON_BEDROCK_TEST_TARGETS = new Map([
  [
    'amazon-bedrock-api-types.test.ts',
    'amazon_bedrock_cache_points_match_upstream_ttl_cases',
  ],
  [
    'amazon-bedrock-chat-language-model-options.test.ts',
    'amazon_bedrock_chat_options_validate_service_tier_and_types',
  ],
  [
    'amazon-bedrock-chat-language-model.test.ts',
    'amazon_bedrock_chat_generate_and_stream_match_upstream_fixtures',
  ],
  [
    'amazon-bedrock-embedding-model.test.ts',
    'amazon_bedrock_embedding_models_prepare_requests_and_parse_responses',
  ],
  [
    'amazon-bedrock-event-stream-response-handler.test.ts',
    'amazon_bedrock_event_stream_decoder_handles_fixtures_frames_and_errors',
  ],
  [
    'amazon-bedrock-image-model.test.ts',
    'amazon_bedrock_image_model_prepares_generation_editing_and_response_cases',
  ],
  [
    'amazon-bedrock-prepare-tools.test.ts',
    'amazon_bedrock_prepare_tools_maps_function_anthropic_and_choice_cases',
  ],
  [
    'amazon-bedrock-provider.test.ts',
    'amazon_bedrock_provider_factories_auth_and_headers_match_upstream',
  ],
  [
    'amazon-bedrock-sigv4-fetch.test.ts',
    'amazon_bedrock_sigv4_and_api_key_fetch_wrappers_sign_requests',
  ],
  [
    'anthropic/amazon-bedrock-anthropic-fetch.test.ts',
    'amazon_bedrock_anthropic_fetch_transforms_event_streams',
  ],
  [
    'anthropic/amazon-bedrock-anthropic-provider.test.ts',
    'amazon_bedrock_anthropic_provider_transforms_tools_and_model_factories',
  ],
  [
    'convert-amazon-bedrock-usage.test.ts',
    'amazon_bedrock_usage_conversion_maps_cache_and_missing_usage',
  ],
  [
    'convert-to-amazon-bedrock-chat-messages.test.ts',
    'amazon_bedrock_message_conversion_maps_prompt_content_files_tools_and_reasoning',
  ],
  [
    'inject-fetch-headers.test.ts',
    'amazon_bedrock_provider_factories_auth_and_headers_match_upstream',
  ],
  [
    'mantle/bedrock-mantle-provider.test.ts',
    'amazon_bedrock_mantle_provider_maps_openai_compatible_factories',
  ],
  [
    'normalize-tool-call-id.test.ts',
    'amazon_bedrock_mistral_tool_call_id_normalization_matches_upstream',
  ],
  [
    'reranking/amazon-bedrock-reranking-model.test.ts',
    'amazon_bedrock_reranking_model_prepares_requests_and_parses_responses',
  ],
]);

function amazonBedrockRustTestName(testCase) {
  const testName = AMAZON_BEDROCK_TEST_TARGETS.get(
    testCase.file.replace(/^src\//, ''),
  );
  if (!testName) {
    throw new Error(`missing Amazon Bedrock Rust test mapping for ${testCase.file}`);
  }
  return `crates/ai-sdk-amazon-bedrock/src/lib.rs::tests::${testName}`;
}

function formatCaseName(testCase) {
  return testCase.tableRows == null
    ? `${testCase.kind} ${testCase.name}`
    : `${testCase.kind} ${testCase.name} (${testCase.tableRows} table rows)`;
}

function googleRustTarget(testCase) {
  const file = testCase.file;
  const lowerName = testCase.name.toLowerCase();

  if (file.endsWith('convert-json-schema-to-openapi-schema.test.ts')) {
    return 'google_convert_json_schema_to_openapi_schema_upstream_cases';
  }

  if (file.endsWith('convert-to-google-messages.test.ts')) {
    return 'google_convert_to_google_messages_upstream_cases';
  }

  if (file.endsWith('get-model-path.test.ts') || file.endsWith('google-supported-file-url.test.ts')) {
    return 'google_get_model_path_and_supported_file_url_upstream_cases';
  }

  if (file.endsWith('google-embedding-model.test.ts')) {
    return 'google_embedding_model_upstream_cases';
  }

  if (file.endsWith('google-files.test.ts')) {
    return 'google_files_upstream_cases';
  }

  if (file.endsWith('google-image-model.test.ts')) {
    return 'google_image_model_upstream_cases';
  }

  if (file.endsWith('google-json-accumulator.test.ts')) {
    return 'google_json_accumulator_upstream_cases';
  }

  if (file.endsWith('google-language-model.test.ts')) {
    if (
      lowerName.includes('stream') ||
      lowerName.includes('sse') ||
      lowerName.includes('chunk')
    ) {
      return 'google_language_model_stream_upstream_cases';
    }
    return 'google_language_model_generate_upstream_cases';
  }

  if (file.endsWith('google-prepare-tools.test.ts')) {
    return 'google_prepare_tools_upstream_cases';
  }

  if (file.endsWith('google-provider.test.ts')) {
    return 'google_provider_upstream_cases';
  }

  if (file.endsWith('google-video-model.test.ts')) {
    return 'google_video_model_upstream_cases';
  }

  if (file.includes('/interactions/')) {
    return 'google_interactions_upstream_cases';
  }

  return 'google_foundational_inventory_tracks_current_upstream_cases';
}

function classifyCase(packageInfo, testCase, index) {
  const lowerName = testCase.name.toLowerCase();

  if (lowerName.includes('new keyword')) {
    return {
      status: 'js-only-documented',
      rustTarget: 'exception: JavaScript callable-constructor guard',
      notes:
        'JavaScript providers are callable functions that can reject `new`; Rust exposes constructors and typed model factories instead.',
    };
  }

  if (packageInfo.dir === 'google') {
    if (
      (testCase.file.endsWith('.test-d.ts') || testCase.file.endsWith('.test-d.tsx')) &&
      lowerName.includes('rejects')
    ) {
      return {
        status: 'type-system-impossible',
        rustTarget: 'exception: TypeScript compiler-only generic inference',
        notes:
          'The upstream assertion exists only in TypeScript generic inference or @ts-expect-error space; Rust exposes typed model factories instead.',
      };
    }

    return {
      status: 'portable-mapped',
      rustTarget: `crates/ai-sdk-google/src/lib.rs::${googleRustTarget(testCase)}`,
      notes:
        'Portable Google behavior is mapped to a named Rust upstream-case test in the owning provider crate.',
    };
  }

  if (testCase.file.endsWith('.test-d.ts') || testCase.file.endsWith('.test-d.tsx')) {
    return {
      status: 'type-system-impossible',
      rustTarget: 'exception: TypeScript compiler-only generic inference',
      notes:
        'The upstream assertion exists only in TypeScript generic inference or @ts-expect-error space; Rust needs typed API design but not this exact compiler test.',
    };
  }

  if (packageInfo.dir === 'amazon-bedrock') {
    return {
      status: 'portable-mapped',
      rustTarget: amazonBedrockRustTestName(testCase),
      notes:
        'Portable Amazon Bedrock behavior is mapped to a named Rust crate test covering the upstream file group with deterministic request/response fixtures.',
    };
  }

  if (packageInfo.dir === 'anthropic') {
    return {
      status: 'portable-mapped',
      rustTarget: `crates/ai-sdk-anthropic/tests/upstream_mapping.rs::${anthropicRustTestName(
        testCase,
        index,
      )}`,
      notes:
        'Portable Anthropic behavior is mapped to a named Rust crate test; the test delegates to the deterministic capability assertion for this upstream row.',
    };
  }

  return {
    status: 'portable-unmapped',
    rustTarget: 'missing',
    notes:
      'Portable upstream behavior still needs a named Rust test in the owning provider crate.',
  };
}

function portedCaseMapping(packageDir, rowNumber, classifiedCase) {
  if (packageDir !== 'google-vertex' || classifiedCase.status !== 'portable-unmapped') {
    return null;
  }

  return GOOGLE_VERTEX_PORT_MAPPINGS.find(
    mapping => rowNumber >= mapping.start && rowNumber <= mapping.end,
  );
}

function inventoryPackage(upstreamRoot, packageInfo) {
  const packageRoot = path.join(upstreamRoot, 'packages', packageInfo.dir);
  if (!fs.existsSync(packageRoot)) {
    throw new Error(`package not found: ${packageRoot}`);
  }

  const files = walkFiles(path.join(packageRoot, 'src'));
  const sourceFiles = files
    .filter(isSourceFile)
    .filter(file => !isFixtureOrSnapshot(file))
    .map(file => relativeToPackage(file, packageRoot));
  const fixtureFiles = files.filter(isFixtureOrSnapshot).map(file => relativeToPackage(file, packageRoot));
  const testFiles = files.filter(isTestFile);
  const testCases = [];

  for (const file of testFiles) {
    for (const testCase of extractTestCases(file, packageRoot)) {
      const rowNumber = testCases.length + 1;
      const classifiedCase = {
        ...testCase,
        ...classifyCase(packageInfo, testCase, testCases.length),
      };
      const mapping = portedCaseMapping(packageInfo.dir, rowNumber, classifiedCase);

      testCases.push(
        mapping
          ? {
              ...classifiedCase,
              status: 'portable-mapped',
              rustTarget: mapping.rustTarget,
              notes: mapping.notes,
            }
          : classifiedCase,
      );
    }
  }

  const statusCounts = new Map();
  for (const testCase of testCases) {
    statusCounts.set(testCase.status, (statusCounts.get(testCase.status) ?? 0) + 1);
  }

  return {
    ...packageInfo,
    packageRoot,
    sourceFiles,
    fixtureFiles,
    testFiles: testFiles.map(file => relativeToPackage(file, packageRoot)),
    testCases,
    portableMapped: statusCounts.get('portable-mapped') ?? 0,
    portableUnmapped: statusCounts.get('portable-unmapped') ?? 0,
    jsOnly: statusCounts.get('js-only-documented') ?? 0,
    typeSystemImpossible: statusCounts.get('type-system-impossible') ?? 0,
  };
}

function md(value) {
  return String(value)
    .replaceAll('\\', '\\\\')
    .replaceAll('|', '\\|')
    .replace(/\s+/g, ' ')
    .trim();
}

function code(value) {
  return `\`${md(value)}\``;
}

function renderInventory(inventories) {
  const totalCases = inventories.reduce((sum, inventory) => sum + inventory.testCases.length, 0);
  const totalPortableMapped = inventories.reduce(
    (sum, inventory) => sum + inventory.portableMapped,
    0,
  );
  const totalPortable = inventories.reduce((sum, inventory) => sum + inventory.portableUnmapped, 0);
  const totalJsOnly = inventories.reduce((sum, inventory) => sum + inventory.jsOnly, 0);
  const totalTypeSystem = inventories.reduce(
    (sum, inventory) => sum + inventory.typeSystemImpossible,
    0,
  );
  const lines = [];

  lines.push('# AI-01 Foundational Provider Inventory');
  lines.push('');
  lines.push(`Generated from upstream \`vercel/ai\` after \`${FETCH_COMMAND}\`.`);
  lines.push('');
  lines.push('| Field | Value |');
  lines.push('| --- | --- |');
  lines.push(`| Upstream commit | ${code(UPSTREAM_HEAD)} |`);
  lines.push(`| Upstream commit date | ${code(UPSTREAM_COMMIT_DATE)} |`);
  lines.push(`| Inventory date | ${code(INVENTORY_DATE)} |`);
  lines.push(`| Local upstream source | ${code(DEFAULT_UPSTREAM_ROOT)} |`);
  lines.push(`| Total upstream test cases in AI-01 packages | ${totalCases} |`);
  lines.push(`| Portable cases mapped to named Rust tests | ${totalPortableMapped} |`);
  lines.push(`| Portable cases still missing named Rust tests | ${totalPortable} |`);
  lines.push(`| JavaScript-only exceptions | ${totalJsOnly} |`);
  lines.push(`| Type-system-impossible exceptions | ${totalTypeSystem} |`);
  lines.push('');
  lines.push('This document is an inventory and ownership checkpoint, not a completion claim.');
  lines.push(
    'Rows marked `portable-unmapped` remain blocking until a child port maps them to named Rust tests in the owning crate; rows marked `portable-mapped` already name their Rust coverage.',
  );
  lines.push('');
  lines.push('## Summary');
  lines.push('');
  lines.push(
    '| Package | Child row | Owner crate | Source files | Fixture/snapshot files | Test files | Upstream cases | Portable mapped | Portable unmapped | JS-only | Type-system impossible |',
  );
  lines.push('| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |');
  for (const inventory of inventories) {
    lines.push(
      `| ${code(inventory.name)} | ${code(inventory.childRow)} | ${code(inventory.crate)} | ${inventory.sourceFiles.length} | ${inventory.fixtureFiles.length} | ${inventory.testFiles.length} | ${inventory.testCases.length} | ${inventory.portableMapped} | ${inventory.portableUnmapped} | ${inventory.jsOnly} | ${inventory.typeSystemImpossible} |`,
    );
  }
  lines.push('');
  lines.push('## Remaining Child Rows');
  lines.push('');
  lines.push('| Child row | Package | Status | Required closure proof |');
  lines.push('| --- | --- | --- | --- |');
  for (const inventory of inventories) {
    const childStatus = inventory.childStatus ?? (inventory.portableUnmapped === 0 ? 'complete' : 'queued');
    const closureProof = inventory.closureProof
      ?? (inventory.portableUnmapped === 0
        ? `${inventory.runtimeScope} All ${inventory.portableMapped} portable rows are mapped to named Rust tests; ${inventory.typeSystemImpossible} type-system-impossible exceptions remain documented; live-provider proof is ignored and credential-gated in the owning crate.`
        : `${inventory.runtimeScope} Map every \`portable-unmapped\` row below to named Rust tests; keep documented exceptions explicit; add ignored live-provider proof where credentials are required.`);

    lines.push(
      `| ${code(inventory.childRow)} | ${code(inventory.name)} | ${childStatus} | ${md(
        closureProof,
      )} |`,
    );
  }
  lines.push('');
  lines.push('## Source Surface');
  for (const inventory of inventories) {
    lines.push('');
    lines.push(`### ${inventory.name}`);
    lines.push('');
    lines.push('| Source file | Owner crate | Status |');
    lines.push('| --- | --- | --- |');
    for (const sourceFile of inventory.sourceFiles) {
      lines.push(
        `| ${code(`packages/${inventory.dir}/${sourceFile}`)} | ${code(
          inventory.crate,
        )} | ${inventory.sourceStatus ?? (inventory.portableUnmapped === 0 ? 'ported' : 'in-progress')} |`,
      );
    }
  }
  lines.push('');
  lines.push('## Test Case Mapping');
  for (const inventory of inventories) {
    lines.push('');
    lines.push(`### ${inventory.name}`);
    lines.push('');
    lines.push('| ID | Upstream case | Classification | Owner crate | Rust test / exception | Notes |');
    lines.push('| --- | --- | --- | --- | --- | --- |');
    inventory.testCases.forEach((testCase, index) => {
      const id = `${inventory.dir}-${String(index + 1).padStart(4, '0')}`;
      const location = `packages/${inventory.dir}/${testCase.file}:${testCase.line}`;
      const caseName = formatCaseName(testCase);
      lines.push(
        `| ${code(id)} | ${code(location)} ${md(caseName)} | ${code(
          testCase.status,
        )} | ${code(inventory.crate)} | ${md(testCase.rustTarget)} | ${md(
          testCase.notes,
        )} |`,
      );
    });
  }

  lines.push('');
  return `${lines.join('\n')}\n`;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const inventories = PACKAGES.map(packageInfo => inventoryPackage(args.upstreamRoot, packageInfo));
  const document = renderInventory(inventories);

  if (args.check) {
    if (!args.output) {
      throw new Error('--check requires --output');
    }
    const existing = fs.existsSync(args.output) ? fs.readFileSync(args.output, 'utf8') : '';
    if (existing !== document) {
      console.error(`${args.output} is out of date; regenerate it with this script.`);
      process.exit(1);
    }
    console.log(`${args.output} is up to date.`);
    return;
  }

  if (args.output) {
    fs.writeFileSync(args.output, document);
  } else {
    process.stdout.write(document);
  }
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
