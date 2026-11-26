# SampleProject

A made-up Unity project folder for the scan tests (crates/uis-core/tests/scan.rs). Nothing here is a
real asset: models are text placeholders, prefabs are short YAML with only the documents the scan reads.

| Prefab | Expected class | Why |
|---|---|---|
| Props/Crate | 3d | own MeshRenderer, mesh from Models/Crate.fbx |
| Props/CrateRed | 3d | variant of Crate |
| Characters/Robot | 3d, character | nests Models/Robot.fbx, a humanoid model |
| Art/HouseArt | 3d | own MeshRenderer |
| Buildings/House | 3d | wrapper around Art/HouseArt |
| Buildings/Tower | 3d | two levels down to Art/HouseArt |
| UI/Button | ui | RectTransform |
| FX/Sparkle | fx | ParticleSystem |
| FX/SparkleBig | fx | variant of Sparkle |
| Sprites/Coin | 2d | SpriteRenderer |
| Misc/Empty | empty | nothing to draw |
| Misc/Binary | unknown | not text YAML |
| Loops/LoopA, LoopB | empty | nest each other (cycle guard) |
| Hidden~/Ghost, Props/.Secret | skipped | names Unity hides |
