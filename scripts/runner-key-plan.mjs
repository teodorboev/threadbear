// What setup does about this Mac's backend key (scripts/setup.mjs, step 5;
// docs/protocol.md, "Runner keys"), worked out apart from doing it so a
// unit test can check it (runner/test/unit/runner-keys.test.ts). Node
// built-ins only, like setup.
//
//   envGet  `npx convex env get RUNNER_KEY`: { status, stdout, stderr }
//   nextGet `npx convex env get RUNNER_KEY_NEXT`, the same shape
//   local   what secrets/convex-key holds now ('' for nothing)
//   joining setup's --existing: a Mac joining a deployment another Mac set up
//
// Returns { action, why? }:
//   fail  stop with `why`: nothing is written and RUNNER_KEY is untouched
//   keep  secrets/convex-key is right as it is
//   copy  write the deployment's RUNNER_KEY into secrets/convex-key (the first Mac)
//   set   set RUNNER_KEY to the local key, or a new one (a deployment with none)
//   joined  this Mac has a runner key: leave it, and RUNNER_KEY, alone
// Whether this Mac joined another Mac's Threadbear: setup's --existing, or
// a runner key already in secrets/convex-key (setup run again there without
// the flag). A joining Mac gets no Convex login, deploy key or backend
// deploy from setup, and never the main key: its key comes from the first
// Mac (tb runner add).
export function isJoining({ existing, local }) {
  return Boolean(existing) || (local ?? '').startsWith('tbr_');
}

const unsetOk = (got) => got.status === 0 && !(got.stdout ?? '').trim() && /not found/i.test(got.stderr ?? '');

export function runnerKeyPlan({ envGet, nextGet, local, joining }) {
  // A joining Mac's key comes from the first Mac (tb runner add), and a
  // runner key is never overwritten with the main key, nor made the main key.
  if (joining || (local ?? '').startsWith('tbr_')) return { action: 'joined' };
  // Only a clean "not found" means the deployment has none: a failed read
  // (no network, a bad deploy key) must never look like an empty one, or a
  // Mac would set RUNNER_KEY to a key of its own and become the owner.
  if (envGet.status !== 0) return { action: 'fail', why: "Couldn't read the deployment's RUNNER_KEY (npx convex env get RUNNER_KEY failed), so setup stopped rather than guess. Check the network and the deploy key, then run setup again." };
  const value = (envGet.stdout ?? '').trim();
  const notFound = !value && /not found/i.test(envGet.stderr ?? '');
  if (!value && !notFound) return { action: 'fail', why: "npx convex env get RUNNER_KEY gave nothing back and didn't say it's unset, so setup stopped rather than guess. Run setup again." };
  if (value && local === value) return { action: 'keep' };
  // Mid-rotation (tb runner rotate-owner), secrets/convex-key holds the new
  // key and RUNNER_KEY the old: copying would put the old one back, and
  // --finish would then keep it alive. So no copy or set until it's
  // finished, nor when whether one's under way can't be told.
  if (!nextGet || !unsetOk(nextGet)) return { action: 'fail', why: (nextGet?.status === 0 && (nextGet.stdout ?? '').trim()) ? 'The main key is being rotated (RUNNER_KEY_NEXT is set): finish with tb runner rotate-owner --finish, then run setup again.' : "Couldn't tell whether the main key is being rotated (npx convex env get RUNNER_KEY_NEXT failed), so setup stopped rather than guess. Run setup again." };
  if (value) return { action: 'copy' };
  return { action: 'set' };
}

// What setup prints at the end on a joining Mac (step 9), worked out here so
// a test can check it. Wayne's second Mac (issue #39): the name it printed
// had .local cut off, so it didn't resolve; Remote Login was off, which
// nothing said was needed; and his user name there wasn't the first Mac's.
//
//   localHost  `scutil --get LocalHostName` (the Bonjour name, without .local), or ''
//   hostname   os.hostname()
//   user       this Mac's user name
//   name       --name, if given
//   sshOn      whether something answers on this Mac's port 22
//
// Returns the lines to print, in order.
export function joinSteps({ localHost, hostname, user, name, sshOn }) {
  // The Bonjour name is what other Macs on the network find this one by;
  // os.hostname() can be a DHCP name (Mac.lan) or an address instead.
  const bare = (localHost || hostname || 'this-mac').trim().replace(/\.local\.?$/i, '');
  const host = /^[\d.]+$|:/.test(bare) || /\./.test(bare) ? bare : `${bare}.local`;
  const target = user ? `${user}@${host}` : host;
  const lines = [];
  if (!sshOn) {
    lines.push("Remote Login is off on this Mac, and your first Mac needs it for the next step (it hands this Mac its key over ssh). Turn it on in System Settings > General > Sharing > Remote Login, then carry on.");
  }
  lines.push(`On your first Mac, run: tb runner add ${name ?? bare} --ssh ${target}`);
  lines.push("It gives this Mac's runner a key of its own and the deployment's address, over ssh. Then restart the runner here (tb restart) and its threads show on your phone and your first Mac.");
  // Forwarding off, as tb runner add's own ssh has it (runner-keys.ts SSH_OPTIONS).
  lines.push(`ssh from your first Mac has to work without a password or a question first, as tb runner add can't answer either. Run ssh -o ForwardAgent=no -o ClearAllForwardings=yes -o ForwardX11=no ${target} there once by hand: answer yes to the host key only if its fingerprint matches this Mac's (ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub here), and if it asks for a password, run ssh-copy-id -o ClearAllForwardings=yes ${target}. tb runner add says which it hit if it fails.`);
  lines.push('tb runner check (here) says if a deploy key or Convex login from an earlier setup is still on this Mac, which you should remove.');
  return lines;
}
