# Reserved one-shot capture failure observation

Main run `42084214870` failed the existing opt-in Windows component case
`reserved_one_shot_wav_capture_releases_only_after_owned_exit_and_can_run_again`.
Its shipping Python bridge returned a deadline failure at the first unwrap. The
other 960 node library cases passed; the coordinator library passed, and the
separate Windows JavaScript/native gates completed. The job and main acceptance
remain failed. A green source run elsewhere does not fix or erase this failure.

This test invokes a synthetic WAV producer through the exact shipping one-shot
host, owned process job and microphone reservation. It opens no microphone,
driver or network. It runs twice and requires exact WAV data and reservation
release only after observed owned-job exit. The original execution deadline is
six seconds; the local bridge limit is ten seconds.

The failed log did not include the existing bridge debug timing event. This
change attaches a future-local test subscriber, filtered to `eefn::python`, so
the shipping numeric setup/spawn/exchange/cleanup durations, effective budget and
outcome code can appear in captured test output. It does not install a global
subscriber or print child output, private settings, paths, IDs or parameters.
The failure message also records the iteration and elapsed caller observation.
Early failures before the existing exchange timing event may still need further
phase evidence; absence of that event is not proof of any particular cause.

No shipping code, deadlines, bridge/resource limits, retry policy, test assertion,
iteration count, permissions, source producer, owned exit requirement or test
selection changes. The case is not ignored or retried. This is observation work,
not a proven fix. The original failure remains evidence. Changed-source hosted
validation must run in full; an unrelated passing build is not main acceptance
or physical microphone acceptance. Heavy local runtime testing is not used.
