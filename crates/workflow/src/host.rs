//! Host subpath classification for upstream files that are re-export shims in
//! JavaScript but not portable Rust runtime behavior.

/// Honest Rust-side classification for a `workflow/*` subpath.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostSubpathClassification {
    /// Host-framework binding with no portable runtime counterpart.
    HostBinding,
    /// JavaScript-only module loader or TypeScript language-service glue.
    JsOnly,
}

/// Classification record for an upstream host subpath file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostSubpath {
    /// Upstream source file under `packages/workflow/src`.
    pub source_file: &'static str,
    /// Rust portability classification.
    pub classification: HostSubpathClassification,
    /// Why the facade does not implement this file as runtime behavior.
    pub note: &'static str,
}

/// Upstream workflow host subpath files reviewed for this bucket.
pub const HOST_SUBPATHS: &[HostSubpath] = &[
    HostSubpath {
        source_file: "astro.ts",
        classification: HostSubpathClassification::HostBinding,
        note: "re-export of @workflow/astro host integration",
    },
    HostSubpath {
        source_file: "nest.ts",
        classification: HostSubpathClassification::HostBinding,
        note: "re-export of @workflow/nest host integration",
    },
    HostSubpath {
        source_file: "next.cts",
        classification: HostSubpathClassification::HostBinding,
        note: "CommonJS Next.js plugin bridge",
    },
    HostSubpath {
        source_file: "nitro.ts",
        classification: HostSubpathClassification::HostBinding,
        note: "Nitro plugin/default export bridge",
    },
    HostSubpath {
        source_file: "nuxt.ts",
        classification: HostSubpathClassification::HostBinding,
        note: "Nuxt module/default export bridge",
    },
    HostSubpath {
        source_file: "sveltekit.ts",
        classification: HostSubpathClassification::HostBinding,
        note: "re-export of @workflow/sveltekit host integration",
    },
    HostSubpath {
        source_file: "vite.ts",
        classification: HostSubpathClassification::HostBinding,
        note: "Vite plugin bridge through @workflow/nitro/vite",
    },
    HostSubpath {
        source_file: "typescript-plugin.cts",
        classification: HostSubpathClassification::JsOnly,
        note: "CommonJS TypeScript server plugin loader",
    },
];
