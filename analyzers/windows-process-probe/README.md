# Non-shipped caller process ownership

`codegraph-process-owner` is a repository acceptance utility, built by
`node scripts/language-packs/public-cli-owner.mjs` through `scripts/cargo-agent.sh`.
It reuses the existing `ProcessTree`: a retained process group on macOS and a
pre-resume, kill-on-close Job with tracked real handles on Windows.

The Node acceptance caller owns this utility. It forwards the analyzer's original
stdin/stdout/stderr bytes and EOF; root exit and cleanup status use a separate
temporary directory. Root exit or pipe EOF never releases the tree. Every normal,
failed or timed-out exchange requests native reap and waits for pipe drain/join
and utility close before returning. Cleanup has bounded deadlines and reports
failure independently of the first transport error.

This is not an analyzer entry, launcher, SDK, worker or production supervisor.
No language package may contain it. Both CI platforms build it before running
the real Node descendant regressions in `scripts/tests/public-cli-transport.test.mjs`.
