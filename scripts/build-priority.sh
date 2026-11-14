# Sourced by a remote.sh run after finding Cargo and Node. Builds run in the
# background; the shell and the benchmark it runs afterwards keep normal
# priority. PATH wrappers also catch builds launched by Node or another shell.
threadbear_build_bin="$(mktemp -d "$TMPDIR/threadbear-build-bin.XXXXXX")"
threadbear_perf_build="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/perf-build.sh"
trap 'rm -rf "$threadbear_build_bin"' EXIT
for threadbear_build_tool in cargo swift swiftc xcodebuild; do
  threadbear_build_real="$(command -v "$threadbear_build_tool")" || continue
  # cargo run/bench and swift run can be measurements themselves. Only the
  # compilation commands get the lower priority; executables run normally.
  case "$threadbear_build_tool" in
    cargo) threadbear_build_actions='build|check|clippy|rustc|test' ;;
    swift) threadbear_build_actions='build|test' ;;
    *) threadbear_build_actions='*' ;;
  esac
  threadbear_build_prefix=''
  [ "$threadbear_build_tool" != cargo ] || printf -v threadbear_build_prefix '/bin/bash %q ' "$threadbear_perf_build"
  {
    cat <<'WRAPPER'
#!/bin/bash
action=''
skip=0
for arg in "$@"; do
  if [ "$skip" = 1 ]; then skip=0; continue; fi
  case "$arg" in
    --config|--color|--manifest-path) skip=1 ;;
    +*|-*) ;;
    *) action="$arg"; break ;;
  esac
done
WRAPPER
    [ "$threadbear_build_tool" != cargo ] || printf 'export THREADBEAR_PERF_BUILD_ACTION="$action"\n'
    printf 'case "$action" in\n  %s) ' "$threadbear_build_actions"
    if [ "$(uname -s)" = Darwin ]; then
      printf 'exec /usr/sbin/taskpolicy -b %s%q "$@" ;;\n' "$threadbear_build_prefix" "$threadbear_build_real"
    else
      printf 'exec nice -n 10 %s%q "$@" ;;\n' "$threadbear_build_prefix" "$threadbear_build_real"
    fi
    printf 'esac\nexec %s%q "$@"\n' "$threadbear_build_prefix" "$threadbear_build_real"
  } >"$threadbear_build_bin/$threadbear_build_tool"
  chmod +x "$threadbear_build_bin/$threadbear_build_tool"
done
export PATH="$threadbear_build_bin:$PATH"
