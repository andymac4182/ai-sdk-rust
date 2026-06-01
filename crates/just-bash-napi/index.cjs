"use strict";

const fs = require("node:fs");
const path = require("node:path");

const addonPath = path.join(
  __dirname,
  `just-bash-napi.${process.platform}-${process.arch}.node`,
);

if (!fs.existsSync(addonPath)) {
  throw new Error(
    "Native Just Bash addon not found. Run `npm run build --prefix crates/just-bash-napi` first.",
  );
}

const native = require(addonPath);

module.exports = {
  ...native,
  Bash: native.RustBash,
  RustBash: native.RustBash,
};
