Read docs/customization-frontier.md and docs/handoff-vertex-color-recolor.md
  (skim), plus your memory notes on the animation frontier. The discovery arc
  is done; this session is the big engineering milestone.

  Step 0 (loose end): the research docs and vpkmerge-core/examples/* from the
  experiment sessions are uncommitted. Run cargo fmt + clippy (CI is pedantic
  and includes examples), fix any nits, and commit them first.

  Main goal, hard but valuable: implement the quantized-pose codec for
  .vnmclip_c in morphic - decode m_compressedPoseData into per-bone
  rotation/translation/scale tracks and re-encode byte-compatibly, honoring
  each clip's m_trackCompressionSettings. VRF's AnimationClip.cs
  (ValveResourceFormat repo) documents the format; the KV3 envelope side
  already works in morphic. Definition of done: a cargo test that round-trips
  several real Deadlock clips (decode -> encode -> decode, poses identical;
  add a committed fixture following the morphic fixtures convention) plus a
  decode sanity check against a known clip (yamato ui_hero_select is a
  single-frame pose, good first target).

  The treat, once the codec round-trips: author the first custom Deadlock
  animation. Take Yamato's ui_hero_select clip, programmatically edit the
  pose (something unmistakable - tilt the head and raise one arm, or rotate
  the root bone ~30 degrees), re-encode, and pack it at her reload_idle +
  reload_idle_quick paths (the proven press-R taunt slot from the
  experiments). Stage it as an addon VPK for me to install and verify
  in-game. If the codec turns out bigger than one session, the fallback
  treat is the v4-blob KV3 reader fix (one value-type case in
  morphic/src/kv3/reader.rs) + a custom death-screen color grade addon from
  postprocessing/gamestate/killed.vpost_c.

  Repo rules: clippy pedantic, no em-dashes anywhere, pure Rust (no .NET in
  shipped tools).