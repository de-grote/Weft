# Raycasts and hitboxes



## Raycasts

`w.in_liquid(pos)` knows a liquid's surface, which the block alone doesn't tell. `w.raycast(from, dir, max, Ray::blocks().entities().skip(me))` (`.solid()` for blocks by what stops a body, `.liquids()`) finds what any line hits first, a block or an entity (a turret, whether a mob sees a player), with the face it went in by and where on it: `hit.local` is from the block's or the entity's box's low corner, a block's 0..1, times 16 the pixel (a panel of buttons on one face); `w.looked_at(id, max, ray)` is that from an entity's eyes.

## Hitboxes

What hits an entity hits the parts of its hitbox: `r.entity(&X).hitboxes(Hitboxes::humanoid())` (or `quadruped()`, `.reaching()` for arms held out, `.slim(2.0)`, and your own `Hitboxes::new().head(..).part(..).limb(..)`), boxes in its model's pixels that turn with its body (`EntityState::body_yaw`, lagging behind the head as vanilla's), a head with the look and legs and arms swinging as it walks: run and an arrow may pass between them. Where it can be hit never comes from its model. `hit.part` says which (`hitbox_part::HEAD`, `hitbox_part_id("tail")`), `hit.face` and `hit.local` are that part's (face 4 its front). The part goes on into the hurt: `Damage::part`, and the `part` a step of `chain::DAMAGE`, `MELEE` or `PROJECTILE_DAMAGE` reads in its query, a blow taking the part its striker looks at. Base's arrows hurt half as much again in the head. A kind without parts is hit by its plain box. `/gizmo hitboxes` (F3+B) shows them: white the box it bumps and is pushed by, pink the parts.
