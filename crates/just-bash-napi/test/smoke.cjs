"use strict";

const assert = require("node:assert/strict");
const { Bash, RustBash, planCliInvocation } = require("../index.cjs");

function jbc18_napi_cjs_entrypoint_requires_and_executes_basic_commands() {
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

  result = bash.exec('echo "Hello" > greeting.txt');
  assert.equal(result.exitCode, 0);
  assert.equal(bash.readFile("greeting.txt"), "Hello\n");

  result = bash.exec("grep", { args: ["-n", "seed", "/workspace/input.txt"] });
  assert.equal(result.stdout, "1:seed\n");
  assert.equal(result.exitCode, 0);

  assert.ok(bash.registeredCommandNames().includes("echo"));
  assert.ok(bash.registeredCommandNames().includes("grep"));

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
}

async function jbc18_napi_esm_entrypoint_imports_and_executes_basic_commands() {
  const mod = await import("../index.mjs");
  assert.equal(mod.Bash, mod.RustBash);
  assert.equal(mod.default.Bash, mod.Bash);
  assert.equal(mod.default.planCliInvocation, mod.planCliInvocation);

  const bash = new mod.Bash({
    files: {
      "/workspace/input.txt": "hello from esm\n",
    },
    cwd: "/workspace",
  });
  const result = bash.exec("cat input.txt");
  assert.equal(result.stdout, "hello from esm\n");
  assert.equal(result.stderr, "");
  assert.equal(result.exitCode, 0);
}

function jbc18_napi_cli_planner_matches_upstream_help_version_and_argument_rows() {
  let plan = planCliInvocation(["--help"], "/repo/project", true);
  assert.equal(plan.action, "help");
  assert.equal(plan.scriptSource, "none");
  assert.equal(plan.exitCode, 0);
  assert.match(plan.output, /Usage:/);
  assert.match(plan.output, /--allow-write/);

  plan = planCliInvocation(["-v"], "/repo/project", true);
  assert.equal(plan.action, "version");
  assert.equal(plan.output, "just-bash 1.0.0");

  plan = planCliInvocation(
    ["-c", "echo hello", "--root", "workspace", "--cwd", "/home/./user/../user/project", "--json"],
    "/repo/project",
    true,
  );
  assert.equal(plan.action, "execute");
  assert.equal(plan.scriptSource, "inline");
  assert.equal(plan.script, "echo hello");
  assert.equal(plan.root, "/repo/project/workspace");
  assert.equal(plan.effectiveCwd, "/home/user/project");
  assert.equal(plan.json, true);

  plan = planCliInvocation(["script.sh"], "/repo/project", true);
  assert.equal(plan.scriptSource, "script-file");
  assert.equal(plan.scriptFile, "script.sh");
  assert.equal(plan.virtualScriptFilePath, "/home/user/project/script.sh");

  plan = planCliInvocation(["--root", "/tmp/project"], "/repo/project", false);
  assert.equal(plan.scriptSource, "stdin");
  assert.equal(plan.root, "/tmp/project");

  assert.throws(
    () => planCliInvocation(["-c"], "/repo/project", true),
    /-c requires a script argument/,
  );
  assert.throws(
    () => planCliInvocation(["--unknown-option"], "/repo/project", true),
    /Unknown option: --unknown-option/,
  );
}

function jbc36_napi_cli_bundle_virtual_execution_and_errexit_rows() {
  const bash = new Bash({
    files: {
      "/tmp/input.txt": "one\ntwo\nthree\n",
    },
    cwd: "/tmp",
  });

  let result = bash.exec("echo hello world");
  assert.equal(result.stdout, "hello world\n");
  assert.equal(result.stderr, "");
  assert.equal(result.exitCode, 0);

  result = bash.exec("cat input.txt | wc -l");
  assert.equal(result.stdout.trim(), "3");

  result = bash.exec("echo test > output.txt && cat output.txt");
  assert.equal(result.stdout, "test\n");
  assert.equal(bash.readFile("output.txt"), "test\n");

  result = bash.exec("set -e\nfalse; echo should_not_print");
  assert.equal(result.stdout, "");
  assert.equal(result.stderr, "");
  assert.equal(result.exitCode, 1);

  const plan = planCliInvocation(["-ec", "false; echo should_not_print"], "/repo/project", true);
  assert.equal(plan.errexit, true);
  assert.equal(plan.script, "false; echo should_not_print");
}

async function main() {
  jbc18_napi_cjs_entrypoint_requires_and_executes_basic_commands();
  await jbc18_napi_esm_entrypoint_imports_and_executes_basic_commands();
  jbc18_napi_cli_planner_matches_upstream_help_version_and_argument_rows();
  jbc36_napi_cli_bundle_virtual_execution_and_errexit_rows();
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
