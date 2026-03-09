#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "record" && "${2:-}" == "--help" ]]; then
	printf 'dictate record\n'
	exit 0
fi

printf 'legacy stderr output\n' >&2
exit 1
