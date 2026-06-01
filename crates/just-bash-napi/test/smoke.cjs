"use strict";

const assert = require("node:assert/strict");
const { Bash, RustBash } = require("../index.cjs");

assert.equal(Bash, RustBash);

const bash = new Bash({
  files: {
    "/workspace/input.txt": "seed\n",
  },
  env: {
    NAME: "Ada",
  },
  cwd: "/workspace",
});

assert.equal(bash.getCwd(), "/workspace");
assert.equal(bash.fileExists("input.txt"), true);

let result = bash.exec("echo hello");
assert.equal(result.stdout, "hello\n");
assert.equal(result.stderr, "");
assert.equal(result.exitCode, 0);
assert.equal(result.metadata.backend, "rust-just-bash");
assert.equal(result.metadata.externalSandbox, false);

result = bash.exec("pwd");
assert.equal(result.stdout, "/workspace\n");
assert.equal(result.exitCode, 0);

bash.writeFile("notes.txt", "from helper\n");
assert.equal(bash.readFile("/workspace/notes.txt"), "from helper\n");

result = bash.exec("cat input.txt > output.txt");
assert.equal(result.exitCode, 0);
assert.equal(bash.readFile("output.txt"), "seed\n");

result = bash.exec("mkdir -p sub && pwd", {
  cwd: "/workspace",
  env: { NAME: "Grace" },
});
assert.equal(result.stdout, "/workspace\n");
assert.equal(result.env.NAME, "Grace");
assert.equal(result.env.PWD, "/workspace");

result = bash.exec("pwd", { cwd: "/workspace/sub" });
assert.equal(result.stdout, "/workspace/sub\n");
assert.equal(result.exitCode, 0);

result = bash.exec("false");
assert.equal(result.stdout, "");
assert.equal(result.stderr, "");
assert.equal(result.exitCode, 1);
