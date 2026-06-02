import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const native = require("./index.cjs");

export const Bash = native.Bash;
export const RustBash = native.RustBash;
export const planCliInvocation = native.planCliInvocation;

export default native;
