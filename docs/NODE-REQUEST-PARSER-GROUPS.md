# Node request parser groups

This unreleased parser follow-up passes integrated hosted/source acceptance at
`f3f9ffb` in [run 42857675899](https://github.com/eepyfishy/eef/actions/runs/42857675899):
all four jobs, 1,755 Windows/headless Rust tests, one ignored across 17 groups,
37 native suites and existing extra gates. It changes no wire DTO, workflow
policy, execution permission, dependency or stack setting.

Source `3c83c1c`, run `42852782316`, passed all 1,751 Windows/headless Rust
tests, one ignored across 17 groups, but failed its first native initialization
case: the actual `eefn --help` executable returned Windows stack-overflow status
`3221225725` instead of zero. The test still requires help to avoid runtime or
configuration creation and to hide private environment values. The failed run
and complete log remain retained; it has not passed full acceptance.

The request CLI had grown to 32 variants in one clap-derived enum. Split its
generated builders into input, submission, planning and review enums, combined
through clap's supported [flattened subcommands](https://docs.rs/clap/latest/clap/_derive/).
The groups are internal parser structure: command names, arguments, defaults,
required fields and execution handlers remain the same. Update typed pattern
matches to unwrap those groups; do not add user-visible command nesting.

This reduces the size of each generated parser group. The integrated checkpoint
passes the unchanged native zero-exit requirement and full suites; it does not
identify the exact overflowing frame in the earlier failed source. Existing
parser assertions remain; failure
output now adds only a closed startup-phase observation and stack-overflow flag,
without printing private environment values or raw process output. No thread
stack is enlarged and no helper process, daemon or runtime is added.

Real model/media workflows, installed-node behavior, physical multi-PC tests,
installer/Defender acceptance and stable release readiness remain separate.
