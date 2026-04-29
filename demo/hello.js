// hello.js - a simple greeting module for E2E testing
// This file will be modified by the Agent IDE CLI as part of the E2E test.

function greet(name) {
  return 'Hello ' + name;
}

/**
 * Sum two numbers
 */
function add(a, b) {
  return a + b;
}

/**
 * Say goodbye to someone asynchronously
 */
async function farewell(name) {
  return 'Goodbye ' + name;
}

module.exports = { greet, add, farewell };