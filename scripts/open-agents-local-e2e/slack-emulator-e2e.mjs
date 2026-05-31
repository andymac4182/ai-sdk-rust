import { spawn } from "node:child_process";
import crypto from "node:crypto";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { createEmulator } from "emulate";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "../..");

const SLACK_TOKEN = "xoxb-open-agents-local";
const SLACK_SIGNING_SECRET = "open-agents-local-signing-secret";
const TEAM_ID = "T123";
const CHANNEL_ID = "C000000001";
const USER_ID = "U000000001";
const BOT_USER_ID = "UOPENAGENT";
const BOT_ID = "BOPENAGENT";
const APP_ID = "AOPENAGENT";

const SLACK_SCOPES = [
  "chat:write",
  "channels:read",
  "channels:history",
  "channels:join",
  "channels:write",
  "im:read",
  "im:history",
  "im:write",
  "users:read",
  "team:read",
];

let serviceProcess;
let stoppingService = false;

async function main() {
  const emulatorPort = await freePort();
  const servicePort = await freePort();
  const serviceUrl = `http://127.0.0.1:${servicePort}`;

  const slack = await createEmulator({
    service: "slack",
    port: emulatorPort,
    seed: {
      slack: {
        team: {
          name: "Open Agents Local",
          domain: "open-agents-local",
        },
        channels: [
          {
            name: "general",
            topic: "Open Agents local E2E",
          },
        ],
        oauth_apps: [
          {
            app_id: APP_ID,
            client_id: "1111111111.2222222222",
            client_secret: "open-agents-local-client-secret",
            name: "Open Agents Local",
            redirect_uris: ["http://localhost/open-agents/slack/oauth"],
            scopes: SLACK_SCOPES,
            bot_id: BOT_ID,
            bot_user_id: BOT_USER_ID,
            bot_name: "open-agents",
          },
        ],
        tokens: [
          {
            token: SLACK_TOKEN,
            user_id: USER_ID,
            scopes: SLACK_SCOPES,
            app_id: APP_ID,
            bot_id: BOT_ID,
            bot_user_id: BOT_USER_ID,
          },
        ],
        incoming_webhooks: [
          {
            channel: "general",
            label: "Open Agents Local",
          },
        ],
        signing_secret: SLACK_SIGNING_SECRET,
        strict_scopes: false,
      },
    },
  });

  try {
    serviceProcess = startService(servicePort, slack.url);
    await waitFor(
      async () => {
        if (serviceProcess.exitCode !== null) {
          throw new Error(`service exited before readiness with ${serviceProcess.exitCode}`);
        }
        const response = await fetch(`${serviceUrl}/readyz`).catch(() => undefined);
        return response?.ok === true;
      },
      "open-agents-service readiness",
      120_000,
    );

    console.log(`ok: Slack emulator listening at ${slack.url}`);
    console.log(`ok: open-agents-service local E2E listening at ${serviceUrl}`);

    const firstText = `<@${BOT_USER_ID}> inspect the repo`;
    const first = await slackApi(slack.url, "chat.postMessage", {
      channel: CHANNEL_ID,
      text: firstText,
    });
    await postSlackEvent(serviceUrl, firstText, first.ts, "EvLOCAL001");
    const final = await waitForThreadMessage(
      slack.url,
      first.ts,
      "Fixture agent finished with local sandbox proof",
    );
    console.log(`ok: app mention completed in Slack thread ${first.ts}: ${final.text}`);

    const questionText = `<@${BOT_USER_ID}> ask a question before continuing`;
    const question = await slackApi(slack.url, "chat.postMessage", {
      channel: CHANNEL_ID,
      text: questionText,
    });
    await postSlackEvent(serviceUrl, questionText, question.ts, "EvLOCAL002");
    const prompt = await waitForThreadMessage(
      slack.url,
      question.ts,
      "Should the local fixture continue?",
    );
    console.log(`ok: question prompt posted in Slack thread ${question.ts}: ${prompt.text}`);

    await postSlackInteraction(serviceUrl, question.ts, "ship it");
    const answered = await waitForThreadMessage(
      slack.url,
      question.ts,
      "Fixture agent finished after answer",
    );
    console.log(`ok: direct interaction payload resumed the run: ${answered.text}`);
  } finally {
    await stopService();
    await slack.close();
  }
}

function startService(port, slackUrl) {
  const child = spawn("cargo", ["run", "-p", "open-agents-service"], {
    cwd: repoRoot,
    env: {
      ...process.env,
      OPEN_AGENTS_BIND_ADDR: `127.0.0.1:${port}`,
      OPEN_AGENTS_STATE: "memory",
      OPEN_AGENTS_SANDBOX: "local",
      OPEN_AGENTS_SANDBOX_ROOT: repoRoot,
      OPEN_AGENTS_SLACK_API_URL: `${slackUrl}/api`,
      SLACK_BOT_TOKEN: SLACK_TOKEN,
      SLACK_SIGNING_SECRET: SLACK_SIGNING_SECRET,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  child.stdout.on("data", (chunk) => process.stdout.write(chunk));
  child.stderr.on("data", (chunk) => process.stderr.write(chunk));
  child.once("exit", (code, signal) => {
    if (!stoppingService && code !== 0) {
      process.stderr.write(`open-agents-service exited with code=${code} signal=${signal}\n`);
    }
  });
  return child;
}

async function stopService() {
  if (!serviceProcess || serviceProcess.exitCode !== null) {
    return;
  }
  stoppingService = true;
  serviceProcess.kill("SIGINT");
  await new Promise((resolve) => {
    const timer = setTimeout(() => {
      if (serviceProcess.exitCode === null) {
        serviceProcess.kill("SIGKILL");
      }
      resolve();
    }, 5_000);
    serviceProcess.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

async function postSlackEvent(serviceUrl, text, ts, eventId) {
  const payload = {
    type: "event_callback",
    team_id: TEAM_ID,
    api_app_id: APP_ID,
    event_id: eventId,
    event_time: Math.floor(Date.now() / 1000),
    event: {
      type: "app_mention",
      user: USER_ID,
      text,
      channel: CHANNEL_ID,
      team: TEAM_ID,
      ts,
    },
  };
  const body = JSON.stringify(payload);
  const response = await fetch(`${serviceUrl}/slack/events`, {
    method: "POST",
    headers: slackSignedHeaders(body, "application/json"),
    body,
  });
  if (!response.ok) {
    throw new Error(`Slack event callback failed: ${response.status} ${await response.text()}`);
  }
}

async function postSlackInteraction(serviceUrl, threadTs, value) {
  const payload = {
    type: "block_actions",
    user: { id: USER_ID, username: "admin" },
    channel: { id: CHANNEL_ID },
    message: { ts: threadTs, thread_ts: threadTs },
    actions: [
      {
        type: "button",
        action_id: "open_agents_answer",
        value,
      },
    ],
  };
  const body = new URLSearchParams({ payload: JSON.stringify(payload) }).toString();
  const response = await fetch(`${serviceUrl}/slack/interactions`, {
    method: "POST",
    headers: slackSignedHeaders(body, "application/x-www-form-urlencoded"),
    body,
  });
  if (!response.ok) {
    throw new Error(`Slack interaction callback failed: ${response.status} ${await response.text()}`);
  }
}

function slackSignedHeaders(body, contentType) {
  const timestamp = Math.floor(Date.now() / 1000).toString();
  const signatureBase = `v0:${timestamp}:${body}`;
  const signature = crypto
    .createHmac("sha256", SLACK_SIGNING_SECRET)
    .update(signatureBase)
    .digest("hex");
  return {
    "Content-Type": contentType,
    "X-Slack-Request-Timestamp": timestamp,
    "X-Slack-Signature": `v0=${signature}`,
  };
}

async function slackApi(slackUrl, method, body) {
  const response = await fetch(`${slackUrl}/api/${method}`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${SLACK_TOKEN}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });
  const json = await response.json();
  if (!response.ok || json.ok !== true) {
    throw new Error(`${method} failed: ${response.status} ${JSON.stringify(json)}`);
  }
  return json;
}

async function waitForThreadMessage(slackUrl, threadTs, expectedText) {
  return waitFor(
    async () => {
      const response = await slackApi(slackUrl, "conversations.replies", {
        channel: CHANNEL_ID,
        ts: threadTs,
      });
      return response.messages.find((message) => message.text?.includes(expectedText));
    },
    `Slack thread ${threadTs} to contain ${JSON.stringify(expectedText)}`,
    30_000,
  );
}

async function waitFor(probe, label, timeoutMs) {
  const started = Date.now();
  let lastError;
  while (Date.now() - started < timeoutMs) {
    try {
      const value = await probe();
      if (value) {
        return value;
      }
    } catch (error) {
      lastError = error;
    }
    await delay(250);
  }
  throw new Error(`Timed out waiting for ${label}${lastError ? `: ${lastError.message}` : ""}`);
}

async function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = address.port;
      server.close(() => resolve(port));
    });
  });
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

main().catch(async (error) => {
  console.error(error);
  await stopService();
  process.exitCode = 1;
});
