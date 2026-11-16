// Every GitHub Action this repo's workflows run is pinned to a full commit
// SHA, and the Android release's signing key only meets a job that checks
// nothing out and runs no Gradle (Daybreak 123, 15 Nov 2026: the release job
// ran actions/checkout@v4 and setup-java@v4, tags anyone who takes over the
// action can move, in the job holding the key and a write token).
//
// To move an action on: look up the tag's commit
// (`gh api repos/<owner>/<repo>/commits/<tag> --jq .sha`), read what
// changed, and put that SHA in with the tag in a comment.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const workflowsDir = path.join(repoRoot, '.github/workflows');

/** Every `uses:` value in a workflow's text, with its line number. */
export function usesOf(text: string): { line: number; ref: string }[] {
  const out: { line: number; ref: string }[] = [];
  text.split('\n').forEach((line, i) => {
    const m = /^\s*(?:-\s+)?uses:\s*(['"]?)([^\s'"#]+)\1/.exec(line);
    if (m) out.push({ line: i + 1, ref: m[2] });
  });
  return out;
}

/** Why a `uses:` value isn't pinned, or null when it is. */
export function unpinned(ref: string): string | null {
  // An action in this repo comes with the commit being run.
  if (ref.startsWith('./')) return null;
  if (ref.startsWith('docker://')) return /@sha256:[0-9a-f]{64}$/.test(ref) ? null : 'a docker image not pinned by its sha256 digest';
  const at = ref.lastIndexOf('@');
  if (at < 0) return 'no ref at all';
  return /^[0-9a-f]{40}$/.test(ref.slice(at + 1)) ? null : `"${ref.slice(at + 1)}" is a tag or branch, which can be moved`;
}

/** A workflow's jobs, each as its own text (two-space-indented keys under `jobs:`). */
export function jobsOf(text: string): Map<string, string> {
  const body = text.slice(text.search(/^jobs:\s*$/m));
  const jobs = new Map<string, string>();
  const heads = [...body.matchAll(/^ {2}([A-Za-z0-9_-]+):\s*$/gm)];
  heads.forEach((m, i) => jobs.set(m[1], body.slice(m.index, heads[i + 1]?.index ?? body.length)));
  return jobs;
}

function workflows(): [string, string][] {
  return fs
    .readdirSync(workflowsDir)
    .filter((f) => /\.ya?ml$/.test(f))
    .map((f) => [f, fs.readFileSync(path.join(workflowsDir, f), 'utf8')]);
}

test('the check itself tells a pinned action from a movable one', () => {
  assert.equal(unpinned('actions/checkout@11d5960a326750d5838078e36cf38b85af677262'), null);
  assert.equal(unpinned('actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9'), null);
  assert.equal(unpinned('./.github/actions/local'), null);
  assert.match(unpinned('actions/checkout@v4')!, /can be moved/);
  assert.match(unpinned('dtolnay/rust-toolchain@stable')!, /can be moved/);
  // A short SHA is a prefix GitHub resolves like a ref, not a pin.
  assert.match(unpinned('actions/checkout@11d5960')!, /can be moved/);
  assert.match(unpinned('docker://alpine:3')!, /digest/);
  assert.deepEqual(usesOf("      - uses: 'actions/setup-java@v4' # v4\n        uses: actions/x@abc"), [
    { line: 1, ref: 'actions/setup-java@v4' },
    { line: 2, ref: 'actions/x@abc' },
  ]);
});

test('every action in every workflow is pinned to a full commit SHA', () => {
  const bad: string[] = [];
  let seen = 0;
  for (const [file, text] of workflows()) {
    for (const { line, ref } of usesOf(text)) {
      seen++;
      const why = unpinned(ref);
      if (why) bad.push(`.github/workflows/${file}:${line} ${ref}: ${why}`);
    }
  }
  assert.ok(seen > 5, 'found no actions at all, so the check is reading the wrong place');
  assert.deepEqual(bad, [], 'Pin these to a commit SHA, with the tag in a comment');
});

test('the Android signing key only meets a job that runs no actions, no checkout and no Gradle', () => {
  // In any workflow, not only android.yml.
  const keyed: string[] = [];
  for (const [file, text] of workflows()) {
    for (const [name, body] of jobsOf(text)) if (/secrets\.ANDROID_KEYSTORE/.test(body)) keyed.push(`${file}:${name}`);
  }
  assert.deepEqual(keyed, ['android.yml:release'], 'only the release job names the key or its password');
  const jobs = jobsOf(fs.readFileSync(path.join(workflowsDir, 'android.yml'), 'utf8'));
  const release = jobs.get('release')!;
  assert.deepEqual(usesOf(release), [], 'the signing job runs no actions at all');
  assert.doesNotMatch(release, /checkout|gradlew|sdkmanager|setup-java/, 'the signing job runs no repo code and downloads no tools');
  // Only on Mike's repo's release tags, as the build is.
  for (const job of [release, jobs.get('apk')!]) {
    assert.match(job, /^ {4}if: github\.repository == 'mikecann\/threadbear' && startsWith\(github\.ref, 'refs\/tags\/release\/'\)/m);
  }
  // The step that signs compares the APK with the build's digest first, in
  // the same step, so nothing runs between the check and the signing.
  const steps = release.split(/\n {6}- /);
  const signing = steps.filter((st) => /apksigner" sign/.test(st));
  assert.equal(signing.length, 1);
  const sign = signing[0];
  assert.match(sign, /WANT: \$\{\{ needs\.apk\.outputs\.sha256 \}\}/);
  const check = sign.search(/\[ "\$got" = "\$WANT" \] \|\| \{[^}]*exit 1; \}/);
  const signs = sign.indexOf('apksigner" sign');
  assert.ok(check > 0 && check < signs, 'the digest is checked before apksigner signs');
  assert.match(sign, /got=\$\(sha256sum "\$unsigned"/);
  assert.match(sign, /--out "\$RUNNER_TEMP\/threadbear-android\.apk" "\$unsigned"/, 'it signs the file it checked');
  // The build job has the Firebase config and a read-only token, never the key.
  const build = jobs.get('apk')!;
  assert.match(build, /contents: read/);
  assert.doesNotMatch(build, /contents: write|KEYSTORE/);
  assert.match(build, /persist-credentials: false/);
});

test("the Gradle wrapper's download is pinned by its digest", () => {
  const props = fs.readFileSync(path.join(repoRoot, 'android-native/gradle/wrapper/gradle-wrapper.properties'), 'utf8');
  assert.match(props, /^distributionSha256Sum=[0-9a-f]{64}$/m);
});
