// The commands a patch that loads without a hash (~/.threadbear/patches,
// Threadbear's own checkout) can run, worked out from where it runs them,
// for writeRunsAsCode (runner/patches.ts).
//
// Outside Full access an edit of a file such a patch runs has to ask, and
// when the runner can't tell what one runs, every edit asks. It used to read
// every string in the patch as a possible command, so a URL, a regex, a
// prompt or the pieces of a quoting helper (`'\''`) were each "a command it
// can't follow", and a home patch written the way the guide shows made every
// edit in every Ask and Auto thread ask (Ville, 11 Nov 2026). Here only what
// reaches somewhere a command runs counts:
//
// - a call to `sh` (the `sh` a live function or `items` is handed, by any
//   path: `sh(...)`, `ctx.sh(...)`, `{ sh: run }` then `run(...)`);
// - an object's `run`, `setup`, `check` or `teardown` (an action's or
//   hook's command, a worktree's), anywhere in the file;
// - `exec`, `execSync`, `execFile`, `execFileSync`, `spawn`, `spawnSync`
//   and `fork` from node:child_process, and those through `promisify`.
//
// Each is worked out from the code as far as it plainly can: strings,
// templates and `+`, constants (a name declared once in the file, `const`),
// `path.join` and friends, `import.meta.dirname`, `fileURLToPath`,
// `homedir()`, a one-line helper like `quote(s)`, `[...].join(' ')`, both
// sides of `a ? b : c`. Anything else is a value worked out as it runs,
// which the command follower (script-trust.ts) treats as `$VALUE`: fine as
// a plain argument (`gh pr view ${n}`), and something it can't follow where
// it picks what runs (`node ${script}`, `${bin} --help`, `sh -c ${x}`).
//
// What this can't see it doesn't guess at: anything that could run a
// command some other way (sh or a child_process function passed around or
// renamed, a package that isn't one of Node's own inert ones, a computed
// property name, a getter) makes the whole file `unknown`, and the caller
// reads every string in it as a command, as before. So a patch is never
// followed less than it was, only more exactly where its shape is plain.
//
// Not in scope, before or now: how a patch quotes a value it splices into a
// command (`sh(\`cat ${name}\`)` with a name holding `;`). That's the user's
// own code running as they wrote it; what's checked is which files it runs.
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { parseSync } from 'oxc-parser';

// A value worked out as it runs, in a command: a shell variable nothing
// sets, which the follower treats as unknown wherever it lands.
export const VALUE = '${__threadbear_value}';
// The same as the follower words it in a problem, to say it plainly.
const VALUE_SAID = /`?\$\{?__threadbear_value\}?`?/g;

export type PatchCommand = { text: string; line: number; worktree?: boolean };
// `hard`: what it runs can't be known from the file at all (its default
// export's commands come from elsewhere), so no reading of it vouches for it.
export type PatchCommands = { commands: PatchCommand[] } | { unknown: { line: number; why: string; hard?: boolean } };

// A follower's problem with VALUE in it, in words.
export function sayValue(problem: string): string {
  return problem.replace(VALUE_SAID, 'a value worked out in code');
}

type AstNode = { type: string; start: number; end: number; [field: string]: any };

const COMMAND_FIELDS = new Set(['run', 'setup', 'check', 'teardown']);
const CP_MODULES = new Set(['child_process', 'node:child_process']);
const CP_FUNCTIONS = new Set(['exec', 'execSync', 'execFile', 'execFileSync', 'spawn', 'spawnSync', 'fork']);
// Node's own modules that can't start a process or load code: a patch may
// import these and still be read exactly. Anything else (a package, cluster,
// inspector, worker_threads, vm, module) could run a command this can't see.
const INERT_MODULES = new Set(
  ['fs', 'fs/promises', 'path', 'path/posix', 'url', 'os', 'crypto', 'util', 'events', 'stream', 'stream/promises', 'buffer', 'string_decoder', 'querystring', 'timers', 'timers/promises', 'assert', 'assert/strict', 'readline', 'readline/promises', 'zlib', 'http', 'https', 'net', 'dns', 'dns/promises', 'perf_hooks']
    .flatMap((m) => [m, `node:${m}`]),
);
// child_process options that don't change what runs.
const INERT_OPTIONS = new Set(['encoding', 'stdio', 'timeout', 'maxBuffer', 'windowsHide', 'input', 'killSignal', 'detached', 'signal', 'uid', 'gid', 'serialization', 'windowsVerbatimArguments']);
const PROCESS_RUNNERS = new Set(['binding', '_linkedBinding', 'execve', 'dlopen']);
const PATH_MODULES = new Set(['path', 'node:path', 'path/posix', 'node:path/posix']);
const PATH_FUNCTIONS = new Set(['join', 'resolve', 'dirname', 'basename', 'normalize', 'relative', 'extname']);
const TYPE_KEYS = new Set(['typeAnnotation', 'typeArguments', 'typeParameters', 'returnType', 'superTypeArguments', 'implements']);
const TYPE_ONLY = new Set(['TSInterfaceDeclaration', 'TSTypeAliasDeclaration', 'TSDeclareFunction', 'TSModuleDeclaration']);
const MAX_VARIANTS = 16;
const MAX_DEPTH = 24;
const MAX_STEPS = 20_000;
// Longer than any command a patch writes: past it, the text is a value.
const MAX_TEXT = 4000;

type Binding =
  | { kind: 'const'; init: AstNode }
  | { kind: 'function'; fn: AstNode }
  | { kind: 'import'; source: string; imported: string } // imported: a name, or '*' / 'default'
  | { kind: 'other' };

class Unknown extends Error {
  readonly at: AstNode;
  readonly hard: boolean;
  constructor(at: AstNode, why: string, hard = false) {
    super(why);
    this.at = at;
    this.hard = hard;
  }
}

export function patchCommands(src: string, file: string, root: boolean): PatchCommands {
  const ext = path.extname(file);
  const sourceType = ext === '.cjs' || ext === '.cts' ? 'commonjs' : ext === '.mjs' || ext === '.mts' ? 'module' : 'unambiguous';
  let program: AstNode;
  try {
    const parsed = parseSync(file, src, { sourceType, preserveParens: false });
    if (parsed.errors.length) return { unknown: { line: 1, why: "it doesn't parse" } };
    program = parsed.program as unknown as AstNode;
  } catch {
    return { unknown: { line: 1, why: "it doesn't parse" } };
  }
  const lineAt = (at: number) => src.slice(0, at).split('\n').length;
  try {
    return { commands: new Reader(program, file, root).read().map((c) => ({ ...c, line: lineAt(c.at) })) };
  } catch (e) {
    if (e instanceof Unknown) return { unknown: { line: lineAt(e.at.start), why: e.message, ...(e.hard ? { hard: true } : {}) } };
    throw e;
  }
}

type Found = { text: string; at: number; worktree?: boolean };
type Parent = { node: AstNode; key: string };

class Reader {
  private parents = new Map<AstNode, Parent>();
  private bindings = new Map<string, Binding[]>();
  // Names that are `sh`, or a child_process function (by which), here.
  private shNames = new Set(['sh']);
  private cpNames = new Map<string, string>();
  private cpSpaces = new Set<string>();
  private promisifyNames = new Set<string>();
  private utilSpaces = new Set<string>();
  private pathNames = new Map<string, string>();
  private pathSpaces = new Set<string>();
  private urlNames = new Map<string, string>();
  private osNames = new Map<string, string>();
  private osSpaces = new Set<string>();
  private worktreeObject: AstNode | undefined;
  private calls: AstNode[] = [];
  // Each constant's value, worked out once; and a cap on the work, so a
  // file of constants built from each other can't hold the runner.
  private constValues = new Map<AstNode, string[]>();
  private steps = 0;

  private program: AstNode;
  private file: string;
  private root: boolean;
  constructor(program: AstNode, file: string, root: boolean) {
    this.program = program;
    this.file = file;
    this.root = root;
  }

  read(): Found[] {
    this.index(this.program, undefined);
    this.findWorktree();
    // Aliases: `const run = promisify(exec)`, `const { sh: run } = ctx`.
    for (const [name, list] of this.bindings) {
      if (list.length !== 1) continue;
      const b = list[0];
      if (b.kind === 'const') {
        const init = unwrap(b.init);
        if (init.type === 'CallExpression' && this.isPromisify(init.callee) && init.arguments.length === 1) {
          const of = this.cpFunctionOf(unwrap(init.arguments[0]));
          if (of) this.cpNames.set(name, of);
        }
      }
    }
    // `versions(sh)` to `async function versions(run) { ... run(...) }`:
    // that parameter is sh too, to a fixed point, as one helper hands it to
    // the next.
    for (let grew = true; grew; ) {
      grew = false;
      for (const call of this.calls) {
        const callee = unwrap(call.callee);
        if (callee.type !== 'Identifier') continue;
        const fn = this.localFunction(callee.name);
        if (!fn) continue;
        call.arguments.forEach((a: AstNode, i: number) => {
          const arg = unwrap(a);
          if (arg.type !== 'Identifier' || !this.shNames.has(arg.name)) return;
          const p = fn.params[i];
          const id = p && (p.type === 'AssignmentPattern' ? p.left : p);
          if (id?.type === 'Identifier' && !this.shNames.has(id.name)) {
            this.shNames.add(id.name);
            grew = true;
          }
        });
      }
    }
    const out: Found[] = [];
    this.walk(this.program, undefined, out);
    return out;
  }

  // ------------------------------------------------------------ indexing

  private index(node: AstNode, parent: Parent | undefined): void {
    if (TYPE_ONLY.has(node.type)) return;
    if (parent) this.parents.set(node, parent);
    if (node.type === 'CallExpression') this.calls.push(node);
    // A name written to after it's declared isn't what it was declared as
    // (`script = () => ...`; Codex's review): it's never folded.
    if (node.type === 'AssignmentExpression' && node.left.type === 'Identifier') this.bind(node.left.name, { kind: 'other' });
    if (node.type === 'UpdateExpression' && node.argument.type === 'Identifier') this.bind(node.argument.name, { kind: 'other' });
    if (node.type === 'AssignmentExpression' && (node.left.type === 'ObjectPattern' || node.left.type === 'ArrayPattern')) this.bindPattern(node.left);
    switch (node.type) {
      case 'ImportDeclaration': {
        const source = String(node.source.value);
        if (node.importKind === 'type') return;
        this.checkModule(source, node);
        for (const s of node.specifiers ?? []) {
          if (s.importKind === 'type') continue;
          const local = s.local.name as string;
          const imported = s.type === 'ImportNamespaceSpecifier' ? '*' : s.type === 'ImportDefaultSpecifier' ? 'default' : importedName(s);
          this.bind(local, { kind: 'import', source, imported });
          if (CP_MODULES.has(source)) {
            if (imported === '*' || imported === 'default') this.cpSpaces.add(local);
            else if (CP_FUNCTIONS.has(imported)) this.cpNames.set(local, imported);
            else throw new Unknown(node, `it imports ${imported} from ${source}`);
          } else if (source === 'util' || source === 'node:util') {
            if (imported === 'promisify') this.promisifyNames.add(local);
            else if (imported === '*' || imported === 'default') this.utilSpaces.add(local);
          } else if (PATH_MODULES.has(source)) {
            if (imported === '*' || imported === 'default') this.pathSpaces.add(local);
            else this.pathNames.set(local, imported);
          } else if (source === 'url' || source === 'node:url') {
            this.urlNames.set(local, imported);
          } else if (source === 'os' || source === 'node:os') {
            if (imported === '*' || imported === 'default') this.osSpaces.add(local);
            else this.osNames.set(local, imported);
          }
        }
        return;
      }
      case 'ExportNamedDeclaration':
      case 'ExportAllDeclaration':
        if (node.source) {
          // Handed on to a file that calls it by a name this one can't see.
          if (CP_MODULES.has(String(node.source.value))) throw new Unknown(node, 'it passes child_process on');
          this.checkModule(String(node.source.value), node);
        }
        break;
      case 'ImportExpression':
        throw new Unknown(node, 'it imports a module as it runs');
      case 'TSImportEqualsDeclaration':
        throw new Unknown(node, 'it uses import = require');
      case 'VariableDeclaration':
        for (const d of node.declarations) {
          if (node.kind === 'const' && d.id.type === 'Identifier' && d.init) this.bind(d.id.name, { kind: 'const', init: d.init });
          else this.bindPattern(d.id);
        }
        break;
      case 'FunctionDeclaration':
        if (node.id) this.bind(node.id.name, { kind: 'function', fn: node });
        for (const p of node.params) this.bindPattern(p);
        break;
      case 'FunctionExpression':
      case 'ArrowFunctionExpression':
        if (node.type === 'FunctionExpression' && node.id) this.bind(node.id.name, { kind: 'other' });
        for (const p of node.params) this.bindPattern(p);
        break;
      case 'ClassDeclaration':
        if (node.id) this.bind(node.id.name, { kind: 'other' });
        break;
      case 'CatchClause':
        if (node.param) this.bindPattern(node.param);
        break;
    }
    for (const k of Object.keys(node)) {
      if (TYPE_KEYS.has(k)) continue;
      const v = node[k];
      for (const child of Array.isArray(v) ? v : [v]) {
        if (child && typeof child === 'object' && typeof child.type === 'string') this.index(child as AstNode, { node, key: k });
      }
    }
  }

  private checkModule(source: string, at: AstNode): void {
    if (/^\.{1,2}(\/|$)|^\/|^file:/.test(source)) return; // a local file: read on its own
    if (CP_MODULES.has(source) || INERT_MODULES.has(source)) return;
    throw new Unknown(at, `it imports ${source}, which could run commands Threadbear can't see`);
  }

  private bind(name: string, b: Binding): void {
    const list = this.bindings.get(name) ?? [];
    list.push(b);
    this.bindings.set(name, list);
  }

  private bindPattern(p: AstNode | null): void {
    if (!p) return;
    switch (p.type) {
      case 'Identifier':
        this.bind(p.name, { kind: 'other' });
        return;
      case 'ObjectPattern':
        for (const prop of p.properties) {
          if (prop.type === 'RestElement') {
            this.bindPattern(prop.argument);
            continue;
          }
          if (prop.computed) throw new Unknown(prop, 'it takes a property by a name worked out as it runs');
          const key = prop.key.type === 'Identifier' ? prop.key.name : String(prop.key.value);
          const value = prop.value.type === 'AssignmentPattern' ? prop.value.left : prop.value;
          // `{ sh: run }`: `run` is sh here.
          if (key === 'sh' && value.type === 'Identifier') this.shNames.add(value.name);
          this.bindPattern(prop.value);
        }
        return;
      case 'ArrayPattern':
        for (const e of p.elements) this.bindPattern(e);
        return;
      case 'AssignmentPattern':
        return this.bindPattern(p.left);
      case 'RestElement':
        return this.bindPattern(p.argument);
      case 'TSParameterProperty':
        return this.bindPattern(p.parameter);
    }
  }

  // The root's `export default { worktree: { ... } }`, whose setup and check
  // are a worktree's (writeRunsAsCode treats those apart).
  // The root's default export has to be written out where this can see each
  // action's, hook's and worktree's command: an object, its lists arrays (or
  // constants holding them) of objects (or constants, or a one-line helper
  // returning one). Lists from JSON.parse or another file could hold a
  // `run` no object here has (Codex's review).
  private checkExport(): void {
    if (!this.root) return;
    const seen = new Set<AstNode>();
    const literal = (n: AstNode, kind: 'ObjectExpression' | 'ArrayExpression', depth = 0): AstNode => {
      const v = unwrap(n);
      if (v.type === kind) return v;
      if (depth < 8 && v.type === 'Identifier') {
        const b = this.only(v.name);
        if (b?.kind === 'const') {
          // Only if nothing else touches it: `hooks.push(...JSON.parse(x))`
          // adds commands no object here has (Codex's review).
          if (this.valueUses(v.name) > 1) throw new Unknown(v, `its default export's ${v.name} is changed or handed on elsewhere`, true);
          return literal(b.init, kind, depth + 1);
        }
      }
      if (depth < 8 && kind === 'ObjectExpression' && v.type === 'CallExpression' && unwrap(v.callee).type === 'Identifier') {
        const fn = this.localFunction(unwrap(v.callee).name);
        const body = fn && returned(fn);
        if (body) return literal(body, kind, depth + 1);
      }
      throw new Unknown(v, "its default export's commands aren't written out in it", true);
    };
    const objectOf = (n: AstNode) => {
      const o = literal(n, 'ObjectExpression');
      if (seen.has(o)) return o;
      seen.add(o);
      for (const p of o.properties) if (p.type === 'SpreadElement') throw new Unknown(p, "its default export's commands aren't written out in it", true);
      return o;
    };
    for (const st of this.program.body) {
      if (st.type !== 'ExportDefaultDeclaration') continue;
      const d = objectOf(st.declaration);
      for (const p of d.properties) {
        const name = keyName(p);
        if (name === 'actions' || name === 'hooks') {
          for (const e of literal(p.value, 'ArrayExpression').elements) {
            if (!e) continue;
            if (e.type === 'SpreadElement') throw new Unknown(e, "its default export's commands aren't written out in it", true);
            objectOf(e);
          }
        } else if (name === 'worktree') objectOf(p.value);
      }
    }
  }

  // How many times a name is read as a value in the file.
  private valueUses(name: string): number {
    let n = 0;
    for (const [node, parent] of this.parents) {
      if (node.type === 'Identifier' && node.name === name && isValue(node, parent, this.parents)) n++;
    }
    return n;
  }

  private findWorktree(): void {
    if (!this.root) return;
    this.checkExport();
    for (const s of this.program.body) {
      if (s.type !== 'ExportDefaultDeclaration') continue;
      const d = unwrap(s.declaration);
      if (d.type !== 'ObjectExpression') continue;
      for (const p of d.properties) {
        if (p.type === 'Property' && !p.computed && keyName(p) === 'worktree' && unwrap(p.value).type === 'ObjectExpression') this.worktreeObject = unwrap(p.value);
      }
    }
  }

  // ------------------------------------------------------------ sinks

  private walk(node: AstNode, parent: Parent | undefined, out: Found[]): void {
    if (TYPE_ONLY.has(node.type)) return;
    switch (node.type) {
      case 'CallExpression':
        this.call(node, out);
        break;
      case 'TaggedTemplateExpression':
        if (node.tag.type === 'Identifier' && (this.shNames.has(node.tag.name) || this.cpNames.has(node.tag.name))) throw new Unknown(node, `it uses ${node.tag.name} as a tag`);
        break;
      case 'Identifier':
        this.reference(node, parent);
        break;
      case 'MemberExpression':
        this.member(node, parent);
        break;
      case 'Property':
        this.property(node, parent, out);
        break;
      case 'AssignmentExpression':
        if (node.left.type === 'MemberExpression') {
          const name = memberName(node.left);
          if (name === undefined) break;
          if (COMMAND_FIELDS.has(name)) out.push(...this.commandsOf(node.right, node));
          if (name === 'sh') throw new Unknown(node, 'it sets sh');
        }
        break;
      case 'MethodDefinition':
      case 'PropertyDefinition':
        if (!node.computed && COMMAND_FIELDS.has(keyName(node) ?? '')) throw new Unknown(node, `a class has ${keyName(node)}`);
        if (node.computed) throw new Unknown(node, 'a class member has a name worked out as it runs');
        break;
    }
    for (const k of Object.keys(node)) {
      if (TYPE_KEYS.has(k)) continue;
      const v = node[k];
      for (const child of Array.isArray(v) ? v : [v]) {
        if (child && typeof child === 'object' && typeof child.type === 'string') this.walk(child as AstNode, { node, key: k }, out);
      }
    }
  }

  private call(node: AstNode, out: Found[]): void {
    const callee = unwrap(node.callee);
    const args: AstNode[] = node.arguments;
    // CommonJS's way in to child_process, or anything else.
    if ((callee.type === 'Identifier' && callee.name === 'require') || (callee.type === 'MemberExpression' && memberName(callee) === 'require')) throw new Unknown(node, 'it uses require');
    if (args.some((a) => a.type === 'SpreadElement') && (this.isSh(callee) || this.cpFunctionOf(callee))) throw new Unknown(node, 'it hands a command over as a spread');
    if (this.isSh(callee)) {
      if (args[0]) out.push(...this.commandsOf(args[0], node));
      return;
    }
    const cp = this.cpFunctionOf(callee);
    if (cp) out.push(...this.cpCommands(cp, args, node));
  }

  // A use of a name that is sh or a child_process function, other than
  // calling it or declaring it: it's going somewhere this can't follow.
  private reference(node: AstNode, parent: Parent | undefined): void {
    const name = node.name as string;
    const isSh = this.shNames.has(name);
    const isCp = this.cpNames.has(name) || this.cpSpaces.has(name);
    if (!isSh && !isCp) return;
    if (!parent || !isValue(node, parent, this.parents)) return;
    const p = parent.node;
    if (parent.key === 'callee' && p.type === 'CallExpression') return;
    // Not even into an object as `sh`: whatever takes it can call it by
    // any name (`Object.values(o)[0](...)`; Codex's review).
    // Handed to a helper in this file, whose parameter is sh too (read()).
    if (isSh && p.type === 'CallExpression' && parent.key === 'arguments') {
      const callee = unwrap(p.callee);
      const fn = callee.type === 'Identifier' ? this.localFunction(callee.name) : undefined;
      const i = p.arguments.indexOf(node);
      const param = fn?.params[i];
      const id = param && (param.type === 'AssignmentPattern' ? param.left : param);
      if (id?.type === 'Identifier' && this.shNames.has(id.name)) return;
    }
    // `cp.execSync(...)`, handled where it's called.
    if (this.cpSpaces.has(name) && p.type === 'MemberExpression' && parent.key === 'object' && CP_FUNCTIONS.has(memberName(p) ?? '') && this.parents.get(p)?.key === 'callee') return;
    // `promisify(exec)`, as a const's value (an alias, read()).
    if (this.cpNames.has(name) && p.type === 'CallExpression' && parent.key === 'arguments' && this.isPromisify(unwrap(p.callee))) {
      const up = this.parents.get(p);
      if (up?.node.type === 'VariableDeclarator' && up.key === 'init' && up.node.id.type === 'Identifier' && this.cpNames.has(up.node.id.name)) return;
    }
    throw new Unknown(node, `it passes ${name} around`);
  }

  private member(node: AstNode, parent: Parent | undefined): void {
    // Node's own ways to start a process or load native code without
    // child_process.
    if (node.object.type === 'Identifier' && node.object.name === 'process' && PROCESS_RUNNERS.has(memberName(node) ?? '')) throw new Unknown(node, `it uses process.${memberName(node)}`);
    if (node.computed && memberName(node) === undefined) {
      // `x[k](...)`: could be sh under another name.
      if (parent?.key === 'callee') throw new Unknown(node, 'it calls a function picked as it runs');
      return;
    }
    if (memberName(node) !== 'sh') return;
    if (parent?.key === 'callee' && parent.node.type === 'CallExpression') return;
    // `sh` written back over, as an assignment's target, is caught there.
    throw new Unknown(node, 'it passes sh around');
  }

  private property(node: AstNode, parent: Parent | undefined, out: Found[]): void {
    // In a pattern, a property names what's taken out: indexed already.
    if (parent?.node.type === 'ObjectPattern') return;
    if (node.computed) {
      if (keyName(node) === undefined) throw new Unknown(node, 'it has a property named as it runs');
    }
    const name = keyName(node);
    if (name === undefined) return;
    if (name === 'sh' && !node.shorthand && !isFunction(unwrap(node.value))) {
      // `{ sh: run }` builds a context: fine. Anything else named sh is a
      // value handed on that might be called.
      if (unwrap(node.value).type !== 'Identifier' || !this.shNames.has(unwrap(node.value).name)) throw new Unknown(node, 'it hands on something as sh');
    }
    if (!COMMAND_FIELDS.has(name)) return;
    if (node.kind !== 'init') throw new Unknown(node, `it has a ${node.kind}ter for ${name}`);
    const value = unwrap(node.value);
    // A method or function is never run as a command (patches.ts reads
    // only strings), and an `{ run: true }` is no command either.
    if (isFunction(value) || node.method) return;
    const worktree = parent !== undefined && parent.node === this.worktreeObject && (name === 'setup' || name === 'check');
    out.push(...this.commandsOf(node.value, node).map((c) => (worktree ? { ...c, worktree: true } : c)));
  }

  // --------------------------------------------------------- what's run

  private commandsOf(expr: AstNode, at: AstNode): Found[] {
    if (expr.type === 'SpreadElement') throw new Unknown(expr, 'it hands a command over as a spread');
    const v = this.value(expr, new Map(), 0);
    return v.map((text) => ({ text, at: at.start }));
  }

  // A child_process call's options, as far as they change what runs: `cwd`
  // (where a relative script is), `shell`. Anything that could change it
  // some other way (`env` with NODE_OPTIONS, a shell by path, `execPath`,
  // options not written in place) makes it a command this can't follow.
  private options(opts: AstNode | undefined): { cwd: (string | undefined)[]; shell: boolean } | 'unknown' {
    // Run from the runner's own folder unless told otherwise, not the
    // thread's, where sh runs (Codex's review).
    const here = process.cwd();
    if (!opts) return { cwd: [here], shell: false };
    const o = unwrap(opts);
    if (o.type !== 'ObjectExpression') return 'unknown';
    let cwd: (string | undefined)[] = [here];
    let shell = false;
    for (const p of o.properties) {
      if (p.type === 'SpreadElement' || p.computed || p.kind !== 'init') return 'unknown';
      const name = keyName(p);
      const v = unwrap(p.value);
      if (name === 'shell') {
        if (v.type === 'Literal' && (v.value === false || v.value === true)) shell = v.value;
        else return 'unknown';
      } else if (name === 'cwd') cwd = this.value(v, new Map(), 0).map((c) => (c.includes(VALUE) ? c : path.resolve(here, c)));
      else if (!INERT_OPTIONS.has(name ?? '')) return 'unknown';
    }
    return { cwd, shell };
  }

  private cpCommands(fn: string, args: AstNode[], at: AstNode): Found[] {
    if (!args[0]) return [];
    if (fn === 'exec' || fn === 'execSync') {
      // exec(command, options?, callback?)
      const o = this.options(args[1] && !isFunction(unwrap(args[1])) ? args[1] : undefined);
      if (o === 'unknown') return [{ text: VALUE, at: at.start }];
      const cmds = this.commandsOf(args[0], at);
      return o.cwd.flatMap((cd) => cmds.map((c) => (cd === undefined ? c : { ...c, text: `cd ${quoteWord(cd)} && ${c.text}` })));
    }
    // execFile(file, [args], opts), spawn(file, [args], opts), fork(module, [args], opts)
    let list: AstNode | undefined = args[1];
    let opts: AstNode | undefined = args[2];
    if (list && unwrap(list).type === 'ObjectExpression') {
      opts = list;
      list = undefined;
    }
    const raw = this.value(args[0], new Map(), 0);
    const programs = fn === 'fork' ? raw.map((p) => [quoteWord('node'), quoteWord(p)].join(' ')) : raw.map(quoteWord);
    let words: string[][];
    if (!list) words = [[]];
    else {
      const l = unwrap(list);
      if (l.type === 'ArrayExpression') {
        words = [[]];
        for (const e of l.elements) {
          if (!e) continue;
          const each = e.type === 'SpreadElement' ? [VALUE] : this.value(e, new Map(), 0);
          words = product(words, each.map((w) => [w]));
        }
      } else words = [[VALUE]];
    }
    const o = this.options(opts);
    if (o === 'unknown') return [{ text: VALUE, at: at.start }];
    const out: Found[] = [];
    for (const cd of o.cwd) {
      for (const [i, prog] of programs.entries()) {
        for (const w of words) {
          // With a shell, the words are joined and the shell reads them, as Node does.
          // With a shell, Node joins the file and arguments as they are and
          // the shell reads that: the file can be a whole command line.
          const text = o.shell && fn !== 'fork' ? [raw[i], ...w].join(' ') : [prog, ...w.map(quoteWord)].join(' ');
          out.push({ text: cd === undefined ? text : `cd ${quoteWord(cd)} && ${text}`, at: at.start });
          if (out.length > MAX_VARIANTS) return [{ text: VALUE, at: at.start }];
        }
      }
    }
    return out;
  }

  // What an expression can be as text: each way it can come out, with
  // VALUE where it's worked out as it runs.
  private value(node: AstNode, env: Map<string, string[]>, depth: number): string[] {
    if (depth > MAX_DEPTH || ++this.steps > MAX_STEPS) return [VALUE];
    const n = unwrap(node);
    const next = (x: AstNode, e = env) => this.value(x, e, depth + 1);
    switch (n.type) {
      case 'Literal':
        if (typeof n.value === 'string' || typeof n.value === 'number' || typeof n.value === 'boolean') return [String(n.value)];
        if (n.value === null && !n.regex) return ['null'];
        return [VALUE];
      case 'TemplateLiteral': {
        let parts: string[] = [n.quasis[0].value.cooked ?? VALUE];
        n.expressions.forEach((e: AstNode, i: number) => {
          parts = join(join(parts, next(e)), [n.quasis[i + 1].value.cooked ?? VALUE]);
        });
        return parts;
      }
      case 'BinaryExpression': {
        if (n.operator !== '+') return [VALUE];
        // Text only when one side is surely text: `1 + 2` is 3, and a value
        // of unknown type might be a number (Codex's review).
        const l = this.textType(n.left, 0);
        const r = this.textType(n.right, 0);
        if (l === 'string' || r === 'string') return join(next(n.left), next(n.right));
        if (l === 'number' && r === 'number') {
          const a = next(n.left);
          const b = next(n.right);
          if (a.length === 1 && b.length === 1 && a[0] !== VALUE && b[0] !== VALUE) return [String(Number(a[0]) + Number(b[0]))];
        }
        return [VALUE];
      }
      case 'ConditionalExpression':
        return union(next(n.consequent), next(n.alternate));
      case 'LogicalExpression':
        return union(next(n.left), next(n.right));
      case 'SequenceExpression':
        return next(n.expressions[n.expressions.length - 1]);
      case 'AwaitExpression':
        return next(n.argument);
      case 'Identifier': {
        const own = env.get(n.name);
        if (own) return own;
        const b = this.only(n.name);
        if (b?.kind !== 'const') return [VALUE];
        const known = this.constValues.get(b.init);
        if (known) return known;
        // Itself while it's worked out (`const a = a + 'x'` never ends).
        this.constValues.set(b.init, [VALUE]);
        const v = next(b.init, new Map());
        this.constValues.set(b.init, v);
        return v;
      }
      case 'MemberExpression':
      case 'MetaProperty': {
        const meta = this.importMeta(n);
        if (meta !== undefined) return [meta];
        return [VALUE];
      }
      case 'CallExpression':
        return this.callValue(n, env, depth);
      default:
        return [VALUE];
    }
  }

  // Whether an expression is null or undefined, through constants.
  private nullish(node: AstNode, depth: number): boolean {
    const u = unwrap(node);
    if (u.type === 'Literal') return u.value === null && !u.regex;
    if (u.type !== 'Identifier' || depth > MAX_DEPTH) return false;
    if (u.name === 'undefined' && !this.bindings.has('undefined')) return true;
    const b = this.only(u.name);
    return b?.kind === 'const' && this.nullish(b.init, depth + 1);
  }

  // Whether an expression is surely a string, surely a number, or either.
  private textType(node: AstNode, depth: number): 'string' | 'number' | 'unknown' {
    if (depth > MAX_DEPTH) return 'unknown';
    const n = unwrap(node);
    switch (n.type) {
      case 'Literal':
        return typeof n.value === 'string' ? 'string' : typeof n.value === 'number' ? 'number' : 'unknown';
      case 'TemplateLiteral':
        return 'string';
      case 'BinaryExpression': {
        if (n.operator !== '+') return ['-', '*', '/', '%', '**'].includes(n.operator) ? 'number' : 'unknown';
        const l = this.textType(n.left, depth + 1);
        const r = this.textType(n.right, depth + 1);
        return l === 'string' || r === 'string' ? 'string' : l === 'number' && r === 'number' ? 'number' : 'unknown';
      }
      case 'Identifier': {
        const b = this.only(n.name);
        return b?.kind === 'const' ? this.textType(b.init, depth + 1) : 'unknown';
      }
      case 'ConditionalExpression': {
        const a = this.textType(n.consequent, depth + 1);
        return a === this.textType(n.alternate, depth + 1) ? a : 'unknown';
      }
      default:
        return 'unknown';
    }
  }

  private importMeta(n: AstNode): string | undefined {
    if (n.type !== 'MemberExpression' || n.object.type !== 'MetaProperty') return undefined;
    const name = memberName(n);
    if (name === 'url') return pathToFileURL(this.file).href;
    if (name === 'dirname') return path.dirname(this.file);
    if (name === 'filename') return this.file;
    return undefined;
  }

  private callValue(n: AstNode, env: Map<string, string[]>, depth: number): string[] {
    const callee = unwrap(n.callee);
    const args: AstNode[] = n.arguments;
    if (args.some((a) => a.type === 'SpreadElement')) return [VALUE];
    const vals = () => args.map((a) => this.value(a, env, depth + 1));
    // path.join(...) and the like, imported or off a namespace.
    // Only through the import itself: a parameter or local of the same
    // name is something else (Codex's review).
    const imported = (name: string) => !env.has(name) && this.only(name)?.kind === 'import';
    const pathFn = callee.type === 'Identifier' && imported(callee.name) ? this.pathNames.get(callee.name) : callee.type === 'MemberExpression' && !callee.computed && callee.object.type === 'Identifier' && imported(callee.object.name) && this.pathSpaces.has(callee.object.name) ? memberName(callee) : undefined;
    if (pathFn && PATH_FUNCTIONS.has(pathFn)) {
      const fn = (path as unknown as Record<string, (...a: string[]) => string>)[pathFn];
      // A value worked out as it runs could be `..`, absolute or anything,
      // which the real function would fold away: the whole path is one.
      return each(vals(), (a) => {
        if (a.some((x) => x.includes(VALUE))) return VALUE;
        try {
          return fn(...a);
        } catch {
          return VALUE;
        }
      });
    }
    if (callee.type === 'Identifier' && imported(callee.name) && this.urlNames.get(callee.name) === 'fileURLToPath' && args.length === 1) {
      return each(vals(), ([u]) => {
        if (u.includes(VALUE) || !u.startsWith('file:')) return VALUE;
        try {
          return fileURLToPath(u);
        } catch {
          return VALUE;
        }
      });
    }
    const osFn = callee.type === 'Identifier' && imported(callee.name) ? this.osNames.get(callee.name) : callee.type === 'MemberExpression' && !callee.computed && callee.object.type === 'Identifier' && imported(callee.object.name) && this.osSpaces.has(callee.object.name) ? memberName(callee) : undefined;
    if (osFn === 'homedir' && args.length === 0) return [os.homedir()];
    if (callee.type === 'Identifier' && callee.name === 'String' && args.length === 1 && !this.bindings.has('String') && !env.has('String')) return this.value(args[0], env, depth + 1);
    // ['node', script, ...].join(' ')
    if (callee.type === 'MemberExpression' && memberName(callee) === 'join' && unwrap(callee.object).type === 'ArrayExpression') {
      const arr = unwrap(callee.object);
      if (arr.elements.some((e: AstNode | null) => !e || e.type === 'SpreadElement')) return [VALUE];
      const sep = args[0] ? this.value(args[0], env, depth + 1) : [','];
      let out: string[] = [''];
      arr.elements.forEach((e: AstNode, i: number) => {
        if (i > 0) out = join(out, sep);
        // join() writes null and undefined as nothing.
        const empty = this.nullish(e, 0);
        out = join(out, empty ? [''] : this.value(e, env, depth + 1));
      });
      return out;
    }
    // A helper of one expression: `const quote = (s) => \`'${...}'\``.
    if (callee.type === 'Identifier' && !env.has(callee.name)) {
      const fn = this.localFunction(callee.name);
      if (fn) {
        const body = returned(fn);
        if (!body) return [VALUE];
        const inner = new Map<string, string[]>();
        const given = vals();
        fn.params.forEach((p: AstNode, i: number) => {
          const id = p.type === 'AssignmentPattern' ? p.left : p;
          if (id.type === 'Identifier') inner.set(id.name, given[i] ?? [VALUE]);
        });
        // Its other names (outer params, locals) are VALUE or constants.
        return this.value(body, inner, depth + 1);
      }
    }
    return [VALUE];
  }

  // ------------------------------------------------------------ names

  private only(name: string): Binding | undefined {
    const list = this.bindings.get(name);
    return list?.length === 1 ? list[0] : undefined;
  }

  // The function a name is, when it's declared once here as one.
  private localFunction(name: string): AstNode | undefined {
    const b = this.only(name);
    if (b?.kind === 'function') return b.fn;
    if (b?.kind === 'const' && isFunction(unwrap(b.init))) return unwrap(b.init);
    return undefined;
  }

  private isSh(callee: AstNode): boolean {
    if (callee.type === 'Identifier') return this.shNames.has(callee.name);
    return callee.type === 'MemberExpression' && memberName(callee) === 'sh';
  }

  private cpFunctionOf(n: AstNode): string | undefined {
    if (n.type === 'Identifier') return this.cpNames.get(n.name);
    if (n.type === 'MemberExpression' && n.object.type === 'Identifier' && this.cpSpaces.has(n.object.name)) {
      const name = memberName(n);
      return name && CP_FUNCTIONS.has(name) ? name : undefined;
    }
    return undefined;
  }

  private isPromisify(n: AstNode): boolean {
    if (n.type === 'Identifier') return this.promisifyNames.has(n.name);
    return n.type === 'MemberExpression' && n.object.type === 'Identifier' && this.utilSpaces.has(n.object.name) && memberName(n) === 'promisify';
  }
}

// ------------------------------------------------------------ helpers

// Through what doesn't change a value: parentheses, TypeScript's casts.
function unwrap(n: AstNode): AstNode {
  while (['ParenthesizedExpression', 'TSAsExpression', 'TSSatisfiesExpression', 'TSNonNullExpression', 'TSTypeAssertion', 'TSInstantiationExpression'].includes(n.type)) n = n.expression;
  return n;
}

function isFunction(n: AstNode): boolean {
  return n.type === 'ArrowFunctionExpression' || n.type === 'FunctionExpression';
}

// The one expression a function gives back, when that's all it does.
function returned(fn: AstNode): AstNode | undefined {
  if (fn.async || fn.generator) return undefined;
  if (fn.body.type !== 'BlockStatement') return fn.body;
  const body = fn.body.body.filter((s: AstNode) => s.type !== 'EmptyStatement');
  return body.length === 1 && body[0].type === 'ReturnStatement' && body[0].argument ? body[0].argument : undefined;
}

function importedName(s: AstNode): string {
  return s.imported.type === 'Identifier' ? s.imported.name : String(s.imported.value);
}

function keyName(p: AstNode): string | undefined {
  if (p.computed) return p.key.type === 'Literal' && typeof p.key.value === 'string' ? p.key.value : p.key.type === 'TemplateLiteral' && p.key.expressions.length === 0 ? p.key.quasis[0].value.cooked : undefined;
  if (p.key.type === 'Identifier' || p.key.type === 'PrivateIdentifier') return p.key.name;
  if (p.key.type === 'Literal') return String(p.key.value);
  return undefined;
}

function memberName(n: AstNode): string | undefined {
  if (!n.computed) return n.property.type === 'Identifier' ? n.property.name : undefined;
  const p = n.property;
  if (p.type === 'Literal' && typeof p.value === 'string') return p.value;
  if (p.type === 'TemplateLiteral' && p.expressions.length === 0) return p.quasis[0].value.cooked;
  return undefined;
}

// Whether an identifier is read as a value here, not declared, named as a
// property or a label.
function isValue(node: AstNode, parent: Parent, parents: Map<AstNode, Parent>): boolean {
  const p = parent.node;
  const k = parent.key;
  if (p.type === 'MemberExpression' && k === 'property') return !!p.computed;
  if ((p.type === 'Property' || p.type === 'MethodDefinition' || p.type === 'PropertyDefinition') && k === 'key') return !!p.computed;
  if (p.type === 'Property' && k === 'value' && parents.get(p)?.node.type === 'ObjectPattern') return false;
  if (p.type === 'VariableDeclarator' && k === 'id') return false;
  if ((p.type === 'FunctionDeclaration' || p.type === 'FunctionExpression' || p.type === 'ArrowFunctionExpression') && (k === 'params' || k === 'id')) return false;
  if (p.type === 'AssignmentPattern' && k === 'left') return false;
  if (p.type === 'ArrayPattern' || p.type === 'RestElement') return parents.get(p)?.node.type === 'ArrayExpression';
  if (p.type.startsWith('Import') || p.type === 'ExportSpecifier') return p.type === 'ExportSpecifier';
  if (p.type === 'CatchClause' || p.type === 'LabeledStatement' || p.type === 'BreakStatement' || p.type === 'ContinueStatement' || p.type === 'MetaProperty') return false;
  if (p.type === 'ClassDeclaration' && k === 'id') return false;
  return true;
}

function join(a: string[], b: string[]): string[] {
  const out: string[] = [];
  for (const x of a) for (const y of b) out.push(x.length + y.length > MAX_TEXT ? VALUE : x + y);
  return out.length > MAX_VARIANTS ? [VALUE] : out;
}

function union(a: string[], b: string[]): string[] {
  const out = [...new Set([...a, ...b])];
  return out.length > MAX_VARIANTS ? [VALUE] : out;
}

function product(a: string[][], b: string[][]): string[][] {
  const out: string[][] = [];
  for (const x of a) for (const y of b) out.push([...x, ...y]);
  return out.length > MAX_VARIANTS ? [[VALUE]] : out;
}

// Each combination of the arguments' ways, through `fn`.
function each(args: string[][], fn: (a: string[]) => string): string[] {
  let combos: string[][] = [[]];
  for (const a of args) {
    combos = product(combos, a.map((x) => [x]));
    // Too many ways: the result is a value, not the call with an argument lost.
    if (combos.length === 1 && combos[0].length === 1 && combos[0][0] === VALUE && args.length > 1) return [VALUE];
  }
  return [...new Set(combos.map(fn))];
}

// One word as a shell reads it back: VALUE stays a variable, in double
// quotes so it stays one word, as an argv entry does.
function quoteWord(w: string): string {
  return w
    .split(VALUE)
    .map((s) => (s === '' ? '' : `'${s.replace(/'/g, `'\\''`)}'`))
    .join(`"${VALUE}"`);
}

