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

result = bash.exec('echo "Hello from CJS consumer"');
assert.equal(result.stdout, "Hello from CJS consumer\n");
assert.equal(result.exitCode, 0);

const websiteStyle = new Bash({
  files: {
    "/home/user/greeting.txt": "Hello\n",
  },
});
result = websiteStyle.exec('echo "friend" >> greeting.txt');
assert.equal(result.exitCode, 0);
result = websiteStyle.exec("cat greeting.txt");
assert.equal(result.stdout, "Hello\nfriend\n");

const exampleStyle = new Bash({
  files: {
    "/data/sample.json": '{"name":"Alice","age":30,"city":"NYC"}',
  },
  cwd: "/data",
});
result = exampleStyle.exec("cat sample.json | jq -r .name");
assert.equal(result.exitCode, 0);
assert.equal(result.stdout, "Alice\n");
