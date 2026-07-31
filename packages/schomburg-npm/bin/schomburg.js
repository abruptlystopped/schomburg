#!/usr/bin/env node

"use strict";

const { version } = require("../package.json");

const help = `Schomburg is a local-first evidence engine.

This release is pre-alpha scaffolding.
Functional evidence collection is not implemented yet.

Usage:
  schomburg --help
  schomburg --version
`;

const [command] = process.argv.slice(2);

if (command === "--version" || command === "-v") {
  console.log(version);
} else if (command === "--help" || command === "-h" || command === undefined) {
  console.log(help);
} else {
  console.error(`Unknown option: ${command}\n\n${help}`);
  process.exitCode = 1;
}
