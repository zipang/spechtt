#!/bin/sh
# Verify (and optionally install) the runtime dependencies SpeechTT needs.
#
# Usage:
#   ./scripts/setup.sh              # probe, show the plan, ask, install, re-probe
#   ./scripts/setup.sh --check      # probe only; exits 1 when something is missing
#   ./scripts/setup.sh --dry-run    # print the plan and the exact commands
#   ./scripts/setup.sh --yes        # skip the confirmation prompt
#
# The arguments are passed to scripts/setup.ts unchanged. This script only makes
# sure `bun` exists, because the dependency logic itself runs on it — and it
# honours the same precedence: `--check` and `--dry-run` never prompt, and
# `--yes` installs `bun` without asking.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$script_dir/.."

BUN_INSTALL="curl -fsSL https://bun.sh/install | bash"

check_only=false
dry_run=false
assume_yes=false
for argument in "$@"; do
	case "$argument" in
	--check) check_only=true ;;
	--dry-run) dry_run=true ;;
	--yes) assume_yes=true ;;
	esac
done

if ! command -v bun >/dev/null 2>&1; then
	if [ "$check_only" = true ] || [ "$dry_run" = true ]; then
		# A report never changes the machine, and a report has nobody to ask.
		echo "bun is missing. Install it with:"
		echo "  $BUN_INSTALL"
		if [ "$check_only" = true ]; then
			echo "Not ready: bun"
			exit 1
		fi
		exit 0
	fi

	if [ "$assume_yes" = true ]; then
		sh -c "$BUN_INSTALL"
	else
		echo "bun is not installed. Install it with:"
		echo "  $BUN_INSTALL"
		printf "Run that now? [y/N] "
		read -r answer || answer=""
		case "$answer" in
		[yY] | [yY][eE][sS]) sh -c "$BUN_INSTALL" ;;
		*)
			echo "Install bun first, then re-run this script." >&2
			exit 1
			;;
		esac
	fi

	PATH="$HOME/.bun/bin:$PATH"
	export PATH
	if ! command -v bun >/dev/null 2>&1; then
		echo "bun is installed but not on PATH yet; open a new terminal and re-run." >&2
		exit 1
	fi
	exec "$script_dir/setup.sh" "$@"
fi

exec bun run scripts/setup.ts "$@"
