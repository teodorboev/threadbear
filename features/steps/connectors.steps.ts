// The Connectors panel (features/connectors.feature). Its lines are what it
// draws: the gateway's machine first, then "<service>: <state>" with its
// button in brackets, and its message under it, indented.
import { Then } from '@cucumber/cucumber';
import type { ThreadbearWorld } from '../support/world.ts';

Then("the Connectors panel's heading is {string}", async function (this: ThreadbearWorld, heading: string) {
  await this.until(`the Connectors panel headed "${heading}"`, (d) => d.connectors?.lines[0] === heading);
});

Then("the Connectors panel doesn't list {string}", async function (this: ThreadbearWorld, line: string) {
  await this.until(`no "${line}" in the Connectors panel`, (d) => d.connectors !== undefined && !d.connectors.lines.includes(line));
});
