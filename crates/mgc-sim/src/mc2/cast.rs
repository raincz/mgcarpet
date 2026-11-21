//! MC2 class-15 CAST MACHINERY + SPELL-XP — the spell column.
//! Ported from the remc2 cast chain (`EF:` = EventsFunctions.cpp,
//! `L:` = Level.cpp cites).
//!
//! Shape: a learned spell IS a class-15 pool entity (the collected
//! jar, re-purposed). Casting is multi-tick: the gate (`sub_5F660`)
//! arms the manifestation's cast timer; the manifestation's EFFECT
//! state (strF0[3·model]) then fires every tick while armed — first
//! tick spawns (the `sub_6DCA0` projectile dispatch or the direct-
//! effect arm) and commits the mana; the timer counts down; expiry
//! applies a pending tier change.
//!
//! Manifestation entity field map (class-15, this module):
//! `word_0x2E_46` armed cast timer → f26 · `word_0x30_48` duration &
//! mana divisor → f28 · `word_0x2C_44` pending tier+1 → f44 ·
//! `byte_0x46_70` live tier → f71 · `subSpellIndex_0x2A_42` payload
//! → f30 · `manaRegen_0x88_136` upkeep → f136 · `maxMana_0x8C_140`
//! full cost → max_life · `mana_0x90_144` per-tick mana → f140 ·
//! `word_0x36_54` cooldown → f54 · `parentId_0x28_40` owner → id24.
//!
//! The per-player `str_611` block lives on [`Mc2Spellbook`] — the
//! human wizard is out-of-pool (chassis convention), so the arrays
//! hang off `World` instead of a `dword_0xA4_164x` record. Single-
//! player laws only: the MP `xpos2` ladder and `sub_6DAD0` are out
//! of scope until rivals cast MC2-natively.
//!
//! Notes:
//! - The mana commit (`sub_68DE0` EF:55569) stamps the full cost as
//!   a negative caster manaRegen; [`World::mana_debit`] is the same
//!   mechanism (MC1's :64936 negative-delta stamp) — the regen tick
//!   applies it next turn, clamped at 0.
//! - Direct-effect spells map onto the existing Player channels
//!   (shield/invisible/rebound/beyond-sight/heal/accelerate/
//!   teleport); per-spell numeric payloads (heal rate, boost factor)
//!   reuse the MC1 channel plumbing pending their own deep trace.
//! - The castle spell (2) routes to [`World::cast_castle`]; retail's
//!   `sub_69AB0` build-queue variant is OPEN; the MC2 mana ladder
//!   (L:1729-55) applies either way.

use crate::engine::features::{
    Gen, no_mc2_m27_v34_lightning_token_residue, no_mc2_m27_v34_meteor_cast_residue,
    no_mc2_m27_v34_token_transparent,
};
use crate::engine::world::{AimLock, Hx, LifeState, PlayerPose, World};
use crate::mc1::mobs::MobCtx;
use crate::mc2::spells::Mc2SubSpell;
use crate::world::PLAYER_LIFE_MAX;

/// Notification lives, in ticks (retail message-life `a3`): the
/// level-up path sets 200 (EF:44012), the change-spell toast 20
/// (EF:37926).
const NOTIFY_TICKS_LEVELUP: u16 = 200;

/// A/B toggle for the Fool's-Mana retaliation AIM PHASE (`sub_36770`
/// EF:26683-86 / `sub_36850` EF:26717-25 aim BEFORE the muzzle lift,
/// and `sub_655C0` raises the target through `sub_65580`): set
/// `MGC_NO_FOOLS_AIM_PHASE` to restore the pre-dig behaviour, where
/// the bolt aimed from its own lifted muzzle at an unraised human.
fn no_fools_aim_phase() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_FOOLS_AIM_PHASE").is_some())
}

/// A/B toggle for THE FOOL'S-MANA FIREBALL'S ABSENT `id_0x1A_26`
/// (`sub_36770` EF:26671-96 vs its sibling `sub_36850` EF:26700-27):
/// set `MGC_NO_FOOLS_BOLT_STRANGER` to restore the pre-dig behaviour,
/// where the fireball inherited the sphere's fused `id24`.
pub(crate) fn no_fools_bolt_stranger() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_FOOLS_BOLT_STRANGER").is_some())
}

/// A/B toggle for THE FOOL'S-MANA WATER-SPLASH ARM (`sub_36770`'s
/// tail, EF:26690-95). The arm is **spawn + sound only, at the
/// SPHERE's own position** — shipped `NETHERW.EXE` file
/// `0x5B014-0x5B045` in full:
///
/// ```text
///   5b014: 6a 05          push $0x5          ; subtype 5
///   5b016: 6a 0a          push $0xa          ; class 10
///   5b018: 57             push %edi          ; &a1x->position_0x4C_76
///   5b019: e8 ..          call _4A190
///   5b01e: 83 c4 0c       add  $0xc,%esp
///   5b021: 85 c0          test %eax,%eax
///   5b023: 74 23          je   0x5b048       ; -> ret
///   5b025..5b039          <pool index of the new record>
///   5b03a: 6a 1b          push $0x1b         ; sound 27
///   5b03c: 6a ff          push $-1
///   5b03f: 50             push %eax
///   5b040: e8 ..          call PrepareEventSound_6E450
/// ```
///
/// — **not one store on the new record**, so the splash keeps
/// `NewEvent_4A050`'s ctor seed `id_0x1A_26 = <own pool index>`
/// (Events.cpp:570) and belongs to nobody, exactly like the
/// retaliation fireball above it ([`no_fools_bolt_stranger`]). The
/// port stamped the sphere's fused `id24` on it; set
/// `MGC_NO_FOOLS_SPLASH_ARM` to restore that.
///
/// ⛔ THE Z HALF OF THIS ARM IS A NON-LAW, CHECKED AND DROPPED.
/// `%edi` is `lea 0x4c(%esi)` computed once at `0x5AF7D` off the
/// SPHERE and pushed for BOTH `_4A190` calls, so retail does place
/// the splash at the sphere's UNLIFTED z while the port passed the
/// fireball's lifted one — but the (10,5) ctor `NewAdd0A05_4E570`
/// (EF:35436-54) ends with
/// `position.z = getTerrainAlt_10C40(&position)`, and the port's
/// `Gen::mc2_spawn_splash` carries the same snap
/// (`mc2/effects.rs:211`). The argument is discarded on both columns.
///
/// ⚠ UNWITNESSED in the current corpus (no take splashes a fool's
/// retaliation) — landed on the disassembly alone, as the same
/// absence class the witnessed `MGC_NO_FOOLS_BOLT_STRANGER` law
/// proved one instruction earlier.
fn no_fools_splash_arm() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_FOOLS_SPLASH_ARM").is_some())
}

/// A/B toggle for THE LIGHTNING RETALIATION'S `id_0x1A_26`
/// (`sub_36850`, shipped `NETHERW.EXE` `0x5B093`
/// `66 8b 40 1a  mov 0x1a(%eax),%ax` / `0x5B097`
/// `66 89 43 1a  mov %ax,0x1a(%ebx)`, EF:26714). `%eax` is `a1x`, the
/// SPHERE, so the bolt copies the sphere's **`@0x1A`** — which for
/// every (10,57) is its own pool slot (`sub_50130`, file `0x74930`,
/// writes @0x45/@0x3F/@0x40/@0x41/@0x42/@0x2C/@0x14/@0x82/@0x38/
/// @0x39/@0x3A/@0x43/@0x90/@0x44 and `0xe |= 2`, and **never @0x1A**,
/// so `NewEvent_4A050`'s own-index seed stands) — and NOT its
/// `parentId_0x28_40`.
///
/// The port's `id24` FUSES `@0x1A` and `@0x28` and the importer
/// resolves it `@0x28`-else-`@0x1A`, so writing `owner` here handed
/// the thunder bolt the Fool's-Mana CASTER, giving it the caster's
/// whole friendly-fire immunity. `Gen::probe_self_id` already unfuses
/// the same lane for (10,57) on the read side (`if id24 != j { j }`),
/// which is exactly the value written here.
/// Set `MGC_NO_FOOLS_LIGHTNING_ID` to restore the pre-dig behaviour.
/// ⚠ UNWITNESSED: no tier-2/3 Fool's Mana is claimed anywhere in the
/// recording corpus.
fn no_fools_lightning_id() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_FOOLS_LIGHTNING_ID").is_some())
}

/// A/B toggle for the CASTLE-DEATH TOKEN PURGE (`sub_605E0`'s level-0
/// rival arm, EF:61645-61658): when a rival castle reaches level 0 on
/// a level whose `byte_0x2FED2 & 4` is set, retail reap-flags whatever
/// record its owner's spell-2 book slot names and zeroes the slot.
///
/// ⭐ **THE COMPANION WAS THE LEVEL GATE, AND IT WAS A STUB.** This
/// law was banked OFF in round 96 as "measured net-negative"
/// (mc2l22 +1 tick, mc2l6-rsg −9,486) — but that measurement was taken
/// with [`World::mc2_castle_purge_level`] hard-wired to `true`, so the
/// arm fired on mc2l6-rsg, which is level **6** (`gfx_type` 0) and
/// where retail's `test byte [eax+0x2fed2],4` FALLS THROUGH. With the
/// real gate wired the arm is inert everywhere but levels 022 and 062,
/// and the regression is gone.
///
/// Set `MGC_NO_MC2_CASTLE_DEATH_TOKEN_PURGE` to restore the pre-dig
/// behaviour (no purge at all).
fn no_mc2_castle_death_token_purge() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CASTLE_DEATH_TOKEN_PURGE").is_some())
}

/// A/B toggle for ⭐⭐ **INVISIBILITY'S CLOAK EDGE ZEROES THE CASTER'S
/// SPAWN GRACE**: set `MGC_NO_MC2_INVIS_CLEARS_GRACE=1` to restore the
/// pre-dig port, which armed the cloak and left `word_0x159_345`
/// standing.
///
/// `sub_6B1C0` (banner `//----- (0006B1C0)` at EF:57418) is the
/// Invisibility manifestation's own tick handler. Its FIRST-TICK arm —
/// `if (sub_68D50(a1x, v1x)) { if (word_0x2E_46 == word_0x30_48) {…} }`,
/// i.e. afforded AND the window's opening tick — writes THREE registers
/// on the CASTER `v1x = Entities_EA3E4[a1x->parentId_0x28_40]`, and the
/// port only modelled two of them:
///
/// ```text
///   v1x->dword_0xA4_164x->word_0x159_345 = 0;            // ← MISSING
///   v1x->dword_0xA4_164x->byte_0x1BF_447 = <tier life>;  // invis_strength
///   v1x->struct_byte_0xc_12_15.byte[0] |= 0x20u;         // the cloak bit
/// ```
///
/// Shipped `NETHERW.EXE`, file = VA + 0x24800, `sub_6B1C0` at 0x8F9C0
/// (`esi` = the caster record, `ebx` = the token):
///
/// ```text
///   8f9f3  e8 58 db ff ff        call 0x8d550        ; sub_68D50(token, caster, 1)
///   8f9fd  0f 84 81 00 00 00     je   0x8fa84        ; not afforded
///   8fa03  66 8b 43 2e           mov  0x2e(%ebx),%ax ; window counter
///   8fa07  66 3b 43 30           cmp  0x30(%ebx),%ax ; == its length?
///   8fa0b  75 5f                 jne  0x8fa6c        ; not the first tick
///   8fa40  8b 86 a4 00 00 00     mov  0xa4(%esi),%eax; the CASTER's player block
///   8fa46  66 c7 80 59 01 00 00  movw $0x0,0x159(%eax)   ; ← THE GRACE CLEAR
///   8fa58  88 81 bf 01 00 00     mov  %al,0x1bf(%ecx)    ; invis strength
///   8fa61  80 cc 20              or   $0x20,%ah          ; the cloak bit
/// ```
///
/// `word_0x159_345` is the spawn-invulnerability / at-castle grace
/// counter ([`crate::engine::world::World::player_mail_block`]'s
/// `grace`, the WIZEXT shadow's `invuln` lane): 100 on a respawn
/// (EF:44063), re-armed to 2 every tick the wizard sits on its own
/// registered castle (`AddPlayer03_00_5E010` EF:60374) and decremented
/// with a full mailbox memset while it is nonzero (EF:60378-81). So
/// **going invisible drops the cloak-caster's damage immunity on the
/// spot** — a fresh respawn's whole 100-tick grace, or the standing
/// at-castle wipe, is gone for that tick.
///
/// ⭐ TWO CALL PATHS, BOTH SEATED: the human's first-tick arm is
/// [`World::mc2_spell_fire`]'s `0xB` (its gate `if afford { if first
/// {…} }` IS retail's pair of tests), and the rival's is
/// [`crate::mc2::rivals::mc2_invis_cloak_edge`]'s `first` branch in
/// `mc2_rival_buffs` — the same handler, reached through the rival
/// book's own countdown loop.
///
/// Witness (free run, so no pair-importer excuse):
/// mc2l6-rival-spells-galore, wiz 0 `invuln` t=18721 / 18900 / 19048,
/// retail 0 port 1 — three human Invisibility casts while parked on
/// the castle, where retail's one-tick dropout is exactly this write
/// and the port held the standing `grace = 1`.
pub(crate) fn no_mc2_invis_clears_grace() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_INVIS_CLEARS_GRACE").is_some())
}

/// A/B toggle for the MC2 HUMAN shield's per-tick CHARGED re-stamp
/// (`sub_6A480` EF:56513 — `parent.byte[1] |= 0x40` on EVERY afforded
/// tick of a `life_0x1A == 0` tier): set `MGC_NO_SHIELD_RESTAMP` to
/// restore the pre-dig behaviour, where `mc2_spell_fire` latched
/// `player.shield` once at the cast and the first absorb spent it for
/// the rest of the window.
/// ⚠⚠⚠ THIS LANE HAD NO KILL SWITCH FOR A WHOLE SESSION AND IT COST A
/// GOLDEN BISECT. `MGC_NO_TOKEN_SLOT_BACKREF=1` restores the pre-dig
/// `word_0x26_38`, which carried a SPELL INDEX (or nothing at all on
/// the fire path).
///
/// Retail's `@0x26` carries the (15,x) token's **POOL SLOT**: both of
/// its readers index `Entities_EA3E4` with it — the impact XP award
/// `sub_6D8B0(a1x->id, Entities[a1x->word_0x26_38]->model_0x40_64, 1)`
/// (EF:62985), for which slot and index happen to be interchangeable
/// because a (15,N) token's model IS N; and the Magic Mine's swallow
/// `sub_68AC0` (EF:55441-44), which reads `v5x->model_0x40_64` AND
/// `v5x->byte_0x46_70` off it and re-arms `v5x->word_0x2E_46 = 1`.
/// The second reader is unreachable from an index, which is why the
/// lane must carry the slot; [`Gen::mc2_token_model`] resolves it back
/// for the first.
///
/// ⚠ Landing it changed `mc2_slice`'s window-D golden — ten human
/// fireballs' `f40` 0 → 152 — with NO observable change (the
/// layout-independent projection holds on all six windows). Because
/// the write was ungated, no kill-switch A/B could attribute that
/// move; it took a per-entity hash probe against a HEAD build.
fn no_token_slot_backref() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_TOKEN_SLOT_BACKREF").is_some())
}

fn no_shield_restamp() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_SHIELD_RESTAMP").is_some())
}

/// ⭐⭐⭐ THE METAMORPH PUPPET IS BORN **HIDDEN** — AN ABSENCE IN AN
/// ENUMERATED LIST, ON THE RECORD THE CALLER'S TAIL STAMPS AND THE
/// CTOR DOES NOT. `sub_6A030`'s spawn-success arm (EF:56325-48) ends
/// with THREE flag writes, not one; the port carried only the second:
/// ```text
///   8e928  80 49 0c 01   or  BYTE PTR [ecx+0xc],0x1   ; the PUPPET
///   8e92c  8a 56 0c      mov dl,BYTE PTR [esi+0xc]
///   8e92f  80 ca 21      or  dl,0x21                  ; the CASTER
///   8e938  88 56 0c      mov BYTE PTR [esi+0xc],dl
///   8e93b  66 8b 40 38   mov ax,WORD PTR [eax+0x38]   ; wizext playerColor
///   8e942  66 3b 42 0c   cmp ax,WORD PTR [edx+0xc]    ; == LevelIndex_0xc?
///   8e946  74 08         je  0x8e950
///   8e948  80 61 0c fe   and BYTE PTR [ecx+0xc],0xfe  ; NON-local: undo it
///   8e94c  80 4e 0c 01   or  BYTE PTR [esi+0xc],0x1
/// ```
/// `ecx` is the record `IfSubtypeCallCreatingManaSphere_4A190` just
/// returned — the same pointer that took `[ecx+0x49]=0xc` (StageVar2
/// 12), `[ecx+0x45]=8*model+7` and `[ecx+0x28]`/`[ecx+0x1a]` six
/// instructions earlier — so the `|= 1` lands on the PUPPET, and it
/// SURVIVES for a LOCAL caster (the `je` skips the `&= 0xfe`).
/// `sub_4C310`, the (5,16) ctor itself (EF:34163-97), contains no such
/// write, so this is the caller's tail and nowhere else — the same
/// shape as round 98's `MGC_NO_SUMMON_HEAD_BEARING`.
///
/// Retail's byte[0] bit 0 is the HIDDEN latch, and it is what
/// [`crate::Gen::mc2_awake_one`] returns on (EF:55515) BEFORE the
/// 0x2400000 proximity test. Without it the port's puppet re-armed
/// `byte_0x39_57 = 16` every 17 ticks forever, and the AWAKE GATE in
/// [`crate::Gen::mc2_aim_scan`] (`kx->byte_0x39_57`, EF:54811) then
/// admitted it as a lock candidate for every class-9 flyer in range.
///
/// WITNESS mc2l6-rival-spells-galore t=28,033: the human's metamorph
/// puppet is slot 171, minted at t=27,925 with retail `flags 13`
/// (bit 0 SET) against the port's 12. Retail's `byte_0x39_57` counts
/// 64 → 0 and then STAYS 0; the port's cycles 16 → 0 → 16. Rival
/// 378's freshly minted (9,0) at slot 306 therefore acquired
/// `target96 = 171` on its BIRTH tick, re-aimed 1730/104 → 1734/113
/// and flew off the launch bearing, where retail's bolt carries
/// `target96 = 0` and flies dead straight (−302, −204, −120 per tick,
/// yaw and pitch frozen) for its whole life.
///
/// Set `MGC_NO_METAMORPH_PUPPET_HIDDEN` to restore the pre-dig
/// behaviour. Round 99 dig 99-18.
pub(crate) fn no_metamorph_puppet_hidden() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_METAMORPH_PUPPET_HIDDEN").is_some())
}

/// A/B toggle for the LAUNCH AIM POINT (`MGC_NO_MC2_LAUNCH_AXIS=1`
/// reverts): the `axis_0x9A_154x` stamp every human cast thunk writes
/// on the flyer it spawns. See [`super::cast::World::mc2_launch`].
pub(crate) fn no_launch_axis() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_LAUNCH_AXIS").is_some())
}

/// A/B toggle for the `subSpellIndex_0x2A_42` ABSENCE on the three
/// direct arms whose retail thunk never writes it
/// (`MGC_NO_MC2_LAUNCH_2A_ABSENCE=1` restores the old unconditional
/// write). See [`super::cast::World::mc2_launch`].
pub(crate) fn no_launch_2a_absence() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_LAUNCH_2A_ABSENCE").is_some())
}

/// ⭐⭐⭐ THE CREATE-CASTLE CAST LOCK IS A **LATCH** (`sub_69AB0`,
/// `sub_5F890`; round 135, W4 — the last head on mc2l3-new and all
/// eight on mc2l5). `MGC_NO_MC2_CASTLE_CAST_LATCH=1` restores the
/// pre-135 arm, which RE-DERIVED the lock every tick from a live scan
/// (a flying human (9,10) or a non-idle human castle) and dropped it
/// the moment the scan went quiet.
///
/// Retail's `word_0x2E_46` on the (15,2) manifestation has exactly
/// these writers, and NOTHING counts it down:
///
/// ```text
///   ARM      sub_5F7B0 (the cast gate)      word = word_0x30_48        (101)
///   PIN      sub_69AB0 VA 0x69BAE           word = word_0x30_48 - 1    (100), gated on
///            the (9,10) spawn returning a slot — `test %eax,%eax / je`
///   RELEASE  sub_69AB0 VA 0x69D42           word = 0, when sub_68D50 is false
///            (the unaffordable fresh cast, a dead/broke caster)
///   PIN/REL  sub_5F890(castle, 1/0)         from the CASTLE's own handlers: pin through
///            the transform (action 5 cases 3/5, action 4 blast-shake, action 6 per
///            level), RELEASE on the settle to standing (action 5 case 2) and the
///            human castle's death (level 0)
///   RELEASE  sub_5F890(ball/carpet, 0)      from the (9,10) BALL's own dispatch (first-
///            tick site refusal EF:58923, payload-spawn failure EF:58877) and the
///            (10,43) delivery that missed the castle (EF:28249)
/// ```
///
/// Once pinned the word HOLDS until one of those releases — in
/// particular across the whole window between the ball's arrival
/// and the castle's settle. That window is where the re-derive was
/// wrong, and `MGC_WRITE_TRACE=<slot>:f26` named it in one run on
/// each take:
///
/// ```text
///   mc2l3-new (token 125 < carpet 167 < castle 176)
///     t=5614  0 -> 101  carpet_dispatch      the cast (button held from here)
///     t=5615  101 -> 100 slot 125 (15,2)     the fire + pin
///     t=5619  100 -> 0   slot 125 (15,2)     ⚠ the RE-DERIVE: ball gone, castle still
///                                            action 4 at the token's slot — retail 100
///     t=5619  0 -> 101   carpet_dispatch     the held button re-arms
///     t=5620  101 -> 100 slot 125 (15,2)     a SECOND (9,10) at slot 197 — the head;
///                                            retail: castle case 0 REFUSES the upgrade
///                                            (sub_11A10), word still 100, no cast
///     t=5621  100 -> 0   slot 176 (3,2)      castle case 2 — retail's release too
///   mc2l5 (carpet 77 < token 80 < castle 174): the same shape one slot-order over —
///     the re-derive releases at 47554, the button re-arms and fires at 47556 (slot
///     195), retail holds 100 through 47556 and releases at 47557 (castle case 2).
/// ```
///
/// Retail's boundary bytes for the whole of both windows read a flat
/// 100 (`dump-state` slots 125 / 80). The eight mc2l5 heads are the
/// same window eight times (a castle whose upgrade keeps being refused
/// under a held button — the castle-through-rock take).
///
/// ⭐⭐⭐ WHY ROUND 132 COULD NOT LAND THIS. Its latch arm lost
/// mc2l1-new at t=3644 (`missing slot 296 (9,10)`), and the table it
/// measured showed retail's word at **0** across the t=3604 cast with
/// a ball "aloft" — read as "retail does not pin on an ordinary cast".
/// The recording's free stack says what actually happened: the ball at
/// slot 296 is allocated at 3604 and FREED by 3605, the human has NO
/// castle, and the ball's own first dispatch (slot 296 > 114, the same
/// tick) fails `sub_11CB0`'s site test and runs the ball-side
/// `sub_5F890(a1x, 0)` release. Arm 101 → pin 100 → release 0, all
/// inside one tick, invisible at the boundary. The pin is real and
/// reached; what the port lacked was the BALL-SIDE release seats —
/// exactly the theory 132 withdrew. They now ride
/// [`crate::engine::features::Gen::mc2_castle_lock_mail`] and land at
/// the ball's own slot (`World::mc2_drain_castle_lock_mail`).
///
/// ⚠ UNIT, NOT PAIR: class-15 `word_0x2E_46` is an ungraded lane, and
/// at every head the pair (importing retail's held 100) buzzes in BOTH
/// arms — the divergence is born a tick or two upstream in the
/// ungraded word. The replay lane certifies it; the unit tests below
/// pin each seat.
pub(crate) fn no_mc2_castle_cast_latch() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CASTLE_CAST_LATCH").is_some())
}

/// A/B toggle for the payload-spawn-failure seat's OWNER-RECORD tail
/// (`sub_6D880(wizard)`, round 135): set
/// `MGC_NO_MC2_CASTLE_BALL_OWNER_TIER_DRAIN=1` to keep that seat a bare
/// release. See `World::mc2_castle_ball_owner_tier_drain`.
pub(crate) fn no_mc2_castle_ball_owner_tier_drain() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CASTLE_BALL_OWNER_TIER_DRAIN").is_some())
}

/// A/B toggle for the MARKER-INDEX castle-lock pin (round 135, mc2l8
/// t=11585): set `MGC_NO_MC2_MARKER_INDEX_PIN=1` to restore the
/// (15,2)-only gate on [`World::mc2_owner_castle_token`], under which a
/// dead wizard's book marker `1` never reaches the class-15 record at
/// pool slot 1.
pub(crate) fn no_mc2_marker_index_pin() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_MARKER_INDEX_PIN").is_some())
}

/// A/B toggle for the marker-index castle-lock stamp's **NON-class-15**
/// arm (round 148, dig w148n): set `MGC_NO_MC2_CASTLE_LOCK_STRAY_2E=1`
/// to restore the class-15-only gate on
/// [`World::mc2_castle_lock_stamp`], under which a book marker of `1`
/// only ever reached pool slot 1 when a class-15 record happened to sit
/// there.
///
/// ⭐⭐⭐ `sub_5F890` HAS NO CLASS TEST AND NO CLAMP. Round 135 landed
/// the marker pin for the class-15 case and this very file's
/// [`World::mc2_owner_castle_token`] doc wrote the rest off — *"A
/// marker that resolves to a NON-class-15 record (mc2l22's (10,45)
/// building at slot 1: retail writes -1 there) stays unlanded"*. It is
/// landable: the port simply has to write retail's `@0x2E` where the
/// port HOMES `@0x2E` for that class (`Ent::f46`, or `Ent::lease2e` on
/// class 5 — `import_ent_mc2`'s `f46` arm and `port_ent_lanes_mc2`'s
/// `f2e` arm), not into `f26`, which on class 10 aliases
/// `dword_0x10_16`.
///
/// Shipped `NETHERW.EXE` (file = VA + 0x24800), `sub_5F890` whole,
/// VA 0x5F890 / file 0x84090:
/// ```text
///   84090  53                      push ebx
///   84091  55                      push ebp
///   84092  89 e5                   mov  ebp,esp
///   84094  8b 55 0c                mov  edx,[ebp+0xc]        ; a1x = the CASTLE
///   84097  0f bf 42 1a             movsx eax,word [edx+0x1a] ; castle->id_0x1A_26 (owner slot)
///   8409b  8b 04 85 e4 a3 01 00    mov  eax,[0x1a3e4+eax*4]  ; Entities[owner]
///   840a2  8b 80 a4 00 00 00       mov  eax,[eax+0xa4]       ; owner->dword_0xA4_164 (player)
///   840a8  66 8b 98 37 03 00 00    mov  bx,[eax+0x337]       ; SpellEnabled[2]  (0x333 + 2*2)
///   840af  66 85 db                test bx,bx
///   840b2  74 35                   je   0x840e9              ; ⭐ THE ONLY GUARD: index != 0
///   840b4  8b 4d 10                mov  ecx,[ebp+0x10]       ; a2
///   840b7  66 85 c9                test cx,cx
///   840ba  75 1a                   jne  0x840d6
///   840bc  0f bf c3                movsx eax,bx              ; --- a2 == 0: RELEASE
///   840bf  8b 04 85 e4 a3 01 00    mov  eax,[0x1a3e4+eax*4]  ; Entities[SpellEnabled[2]]
///   840c6  52                      push edx
///   840c7  66 89 48 2e             mov  [eax+0x2e],cx        ; = 0, UNCONDITIONALLY
///   840cb  e8 b0 df 00 00          call 0x92080              ; sub_6D880(CASTLE)
///   840d6  0f bf c3                movsx eax,bx              ; --- a2 != 0: PIN
///   840d9  8b 04 85 e4 a3 01 00    mov  eax,[0x1a3e4+eax*4]
///   840e0  66 8b 50 30             mov  dx,[eax+0x30]        ; word_0x30_48
///   840e4  4a                      dec  edx                  ; ⭐ PLAIN dec — NO `.max(1)`
///   840e5  66 89 50 2e             mov  [eax+0x2e],dx        ; word_0x2E_46 = @0x30 - 1
/// ```
/// (decompile: `sub_5F890`, resolve by banner `//----- (0005F890)`.)
///
/// **THE MEASURED SHAPE.** `SpellEnabled[k]` is `int16_t[26]` at player
/// +0x333 and is routinely a MARKER, not a slot: the wizard-death
/// handler `sub_5E310` stamps a boolean **1** into every occupied book
/// slot (already documented on `Gen::mc2_devour_rebound` — the READ
/// side of the same marker). So a castle-less wizard carries
/// `spell_ent[2] == 1`, and every castle transform tick runs the stamp
/// against **pool slot 1**, whatever lives there. Slot 1's `@0x30` is 0
/// in every MC2 take in the corpus, so the pin arm writes `0 - 1 =
/// -1` and the release arm writes it back to 0 — a `-1` pulse on pool
/// slot 1's `@0x2E` that the free-run raw shadow sees on FIVE classes
/// across 13 takes, always slot 1, never any other slot.
///
/// WITNESSES (retail, `spell_ent[2]` straight out of the recording):
/// * mc2l4 — rival p1's `spell_ent[2]` 295 → **1** at t=1247 and p2's
///   301 → **1** at t=2204; slot 1 (an (11,4) switch, `@0x30` 0) then
///   takes `f2e 0 → -1` at t=2423, `-1 → 0` at t=2473 (the same tick
///   castle slot 304 settles `f2e 2 → 0` / `action45 5 → 4`, i.e.
///   `BeginOfCastleCreation_5FA70` case 2's `sub_5F890(locEvent, 0)`),
///   `0 → -1` at 2530, `-1 → 0` at 2580, `0 → -1` at 2582.
/// * mc2l16 slot 1 `(14,1)` t=6408 (21,311 rows), mc2l17 slot 1
///   `(5,17)` t=297 and `(5,16)` t=11871 (15,520 rows), mc2l22 slot 1
///   `(5,23)` t=4583 (407), mc2l6 `(5,0)` t=8557 (334),
///   mc2l6-rsg `(10,40)`/`(10,14)` t=6366.. (155), mc2l18 `(10,71)`
///   t=11255 (75), mc2l8/mc2l22 `(10,0)` (21), mc2l6 `(9,3)` (8).
pub(crate) fn no_mc2_castle_lock_stray_2e() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CASTLE_LOCK_STRAY_2E").is_some())
}

/// Where the port homes retail's `word_0x2E_46` for a given class —
/// the mirror of `port_ent_lanes_mc2`'s `f2e` arm.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Raw2eHome {
    /// class 15: the manifestation's armed timer.
    F26,
    /// class 5: [`crate::engine::features::Ent::lease2e`], the summon
    /// lease (`no_summon_lease_field`'s home).
    Lease,
    /// the uniform home.
    F46,
    /// `f46` is repurposed on this family and `@0x2E` has no home at
    /// all — the `(10,{39,57})` sphere, the `(10,45)` link and
    /// `(10,78)` mine (`@0x3D`), the `(10,{76,77})` orb breathe — or
    /// the home is a `u8` that cannot hold `-1` (the class-3 castle's
    /// `f59`). The lane publishes `—` for all of them, so dropping the
    /// write costs no measurable row and cannot corrupt a live field.
    None,
}

pub(crate) fn raw_at_2e_home(c: u8, m: u8) -> Raw2eHome {
    if c == 15 {
        return Raw2eHome::F26;
    }
    if c == 5 {
        return Raw2eHome::Lease;
    }
    if (c == 10 && matches!(m, 39 | 57 | 45 | 78))
        || c == 3
        || crate::engine::world::conformance::orb_breathe_at_3d(c, m)
    {
        return Raw2eHome::None;
    }
    Raw2eHome::F46
}

/// Where the port homes retail's `word_0x30_48` — the mirror of
/// `port_ent_lanes_mc2`'s `f30` arm.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(crate) enum Raw30Home {
    F28,
    F50,
    /// `f50` is spent on another retail word for this family and
    /// `@0x30` is dead 0 across the corpus (the `(5,10)` pyramid's
    /// summon stride, the `(5,27)` hydra spline, the `(10,75)` node).
    Dead,
}

pub(crate) fn raw_at_30_home(c: u8, m: u8) -> Raw30Home {
    if c == 15 {
        return Raw30Home::F28;
    }
    if (c == 5 && matches!(m, 10 | 27)) || (c == 10 && m == 75) {
        return Raw30Home::Dead;
    }
    Raw30Home::F50
}

/// `sub_5F890`'s PIN value — `word_0x30_48` with a plain 16-bit `dec`
/// (`NETHERW.EXE` 0x840E0 `mov dx,[eax+0x30]` / 0x840E4 `dec edx` /
/// 0x840E5 `mov [eax+0x2e],dx`). ⚠ The pre-dig port wrote
/// `f28.max(1) as i16 - 1`, which floors the result at 0 — the
/// protocol's highest-yield tell, and the reason a marker-1 record
/// with `@0x30 == 0` read `port 0` against `retail -1`.
pub(crate) fn castle_lock_pin_value(at30: i16) -> i16 {
    at30.wrapping_sub(1)
}

/// A/B toggle for the RIVAL twin of the CASTLE-SPELL PENDING-TIER
/// ONE-TICK LAG: set `MGC_NO_MC2_RIVAL_CASTLE_TIER_DEFER_LAG=1` to
/// restore the pre-dig [`World::mc2_rival_castle_lock_release`], which
/// applied the manifestation's deferred `word_0x2C_44` tier inside the
/// release's own tick. Retail's `sub_5F890` `a2 == 0` arm runs
/// `sub_6D880` on **the CASTLE**, not the manifestation it just
/// released (NETHERW.EXE file 0x840BC-0x840CB) — and EF:61036 resolves
/// `SpellsEnabled[2]` off `Entities[castle->id_0x1A_26]` with no
/// player test, so the rival column takes the same lag as the human's
/// [`no_mc2_castle_tier_defer_lag`].
pub(crate) fn no_mc2_rival_castle_tier_defer_lag() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_RIVAL_CASTLE_TIER_DEFER_LAG").is_some())
}

/// A/B toggle for the CASTLE-SPELL PENDING-TIER ONE-TICK LAG
/// (`sub_5F890`'s release arm + `sub_69AB0`'s entry arm — see
/// [`World::mc2_castle_spell_tick`]): set
/// `MGC_NO_MC2_CASTLE_TIER_DEFER_LAG` to restore the pre-dig
/// behaviour, where the lock RELEASE applied the manifestation's
/// deferred tier in its own tick and the manifestation's handler had
/// no spent-timer entry arm at all — so the tier stamp landed one tick
/// ahead of retail and, on an import that starts between the two, not
/// at all.
pub(crate) fn no_mc2_castle_tier_defer_lag() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_CASTLE_TIER_DEFER_LAG").is_some())
}

/// ⭐⭐⭐ A/B toggle for **A STOLEN SPELL JAR KEEPS ITS CAST STATE**
/// (`sub_69300` EF:56136 + `sub_59DC0` EF:41199 + `sub_68FF0`
/// EF:56011): set `MGC_NO_MC2_STOLEN_ARC_KEEPS_CAST_STATE` to restore
/// the pre-dig behaviour, where the m26 wraith's steal, the detach
/// arc and the re-collect each clobbered the manifestation's ARMED
/// CAST TIMER (`word_0x2E_46`, port `f26`) because the port aliased
/// the arc counter `dword_0x10_16` onto the SAME field — so a spell
/// stolen mid-cast came back with a dead window and never paid out
/// its remaining burn ticks.
///
/// THE ALIAS WAS THE ROOT. `cast.rs`'s module map says
/// "`word_0x2E_46` armed cast timer → f26" and
/// [`World::mc2_spell_steal`]'s own doc says "f26 = the arc counter
/// (`dword_0x10_16`)" — **the port's two comments name two different
/// retail words for one field, 800 lines apart**, and retail keeps
/// BOTH live at once. The arc counter now lives in `f50` (dead for
/// class 15: `import_ent_mc2` already zeroes it there).
pub(crate) fn no_mc2_stolen_arc_keeps_cast_state() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_STOLEN_ARC_KEEPS_CAST_STATE").is_some())
}

/// `MGC_NO_MC2_STOLEN_ARC_YAW=1` restores the port's yaw-less detach
/// arc AND the `yaw` lane's hard `some(0)` for class 15 — i.e. it
/// reverts this law.
///
/// ⭐⭐⭐ **A COMMENT CLAIMING A LANE IS DELIBERATELY UNMODELLED IS A
/// DIG LEAD.** [`World::mc2_stolen_arc`] stored the caster's PITCH on
/// the detached jar and dismissed the yaw in the same breath — "yaw
/// @0x1C has no port home on a manifestation — f30 carries the
/// subSpell payload — and its obs heading projects 0, so it is
/// skipped" — and `port_ent_lanes_mc2` published the class-15 `yaw`
/// lane as a literal `some(0)` on the matching claim "retail yaw dead
/// 0". The two dismissals covered for each other: the store was
/// missing and the only instrument that could have seen it was pinned
/// to zero.
///
/// Retail's `sub_59DC0` (banner `//----- (00059DC0)`, EF:41504) writes
/// BOTH words on every rising tick of the arc, off the parent
/// (`Entities[parentId_0x28_40]`, the caster):
/// ```text
///   a1x->yaw_0x1C_28   = v1x->yaw_0x1C_28;
///   v3 = v1x->pitch_0x1E_30;
///   a1x->pitch_0x1E_30 = v3;
///   MoveEntity_57FA0(&predicted, v1x->yaw_0x1C_28, v3 - 16 * n, 384);
/// ```
/// The stored yaw is the parent's, unsweeped; only the MOVE bends the
/// pitch by 16·n. Past `dword_0x10_16 > 5` the homing leg writes
/// neither, so the jar keeps the yaw it had at n = 5 for the rest of
/// its life — which is why every witness row is a long constant.
///
/// @0x1C now has a class-15 home: `f46`, dead on class 15 (`f2e`
/// publishes `f26` there; `f2a`/`f2c` borrow `f46` only on the worm /
/// pyramid / sphere / castle families). `import_ent_mc2` seats
/// `r.yaw` into it and the lane publishes it.
///
/// WITNESSES — free run, `MGC_RAW_SHADOW=1`, `yaw` lane, retail live /
/// port 0: `(15,1)` 62,441 rows over 5 takes, `(15,0)` 67,850 over 4,
/// `(15,9)` 9,177 over 1 — e.g. mc2l8 slot 105 `(15,0)` 22,303 rows
/// from t=791 (retail 1808 = the human's yaw at that tick, the (5,26)
/// wraith at slot 11), mc2l5 slot 78 from t=1915 (retail 889),
/// mc2l7 slot 44 `(15,9)` from t=21669 (retail 1838).
pub(crate) fn no_mc2_stolen_arc_yaw() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_STOLEN_ARC_YAW").is_some())
}

/// ⭐⭐ **THE IMPORTER'S LAST LINE STILL SPOKE THE OLD FIELD MAP —
/// EVERY IMPORTED MID-ARC JAR LOST ITS `@0x24` KILLER LATCH.**
///
/// The m26-wraith steal's homing arc `sub_59DC0` reads its wraith back
/// from `@0x26`, not `@0x24`: `sub_69300`'s first store is
/// `mov %ax,0x26(%ebx)` (`NETHERW.EXE` file 0x8DB32; MC2 file =
/// VA + 0x24800) and the arc's homing leg re-reads `0x26(%ebx)` at
/// file 0x7E656. The port moved the wraith ref `f38` -> `f40`
/// accordingly (`mc2/cast.rs` `e.f40 = wraith`), which is where
/// `import_ent_mc2`'s GENERIC seat already puts it
/// (`f40: tr(r.f26 as u16)`) and where `port_ent_lanes_mc2`'s `f26`
/// lane publishes it from.
///
/// What did NOT move is the class-15 `action == 78` tail of
/// `import_ent_mc2`, which still ended with `e.f38 = tr(r.f26 as u16)`
/// — a leftover from the old home. `e.f38` is `@0x24`, seated three
/// hundred lines earlier as `f38: tr(r.f24 as u16)` and published on
/// the `f24` lane ("@0x24 is honestly the killer latch again now that
/// the jar arc's wraith ref lives in `f40`"). So the import CLOBBERED
/// every detached jar's `@0x24` with its `@0x26`, and the lane table
/// and the importer contradicted each other on the one record family
/// where both are live. Pair mode is the only view that can see it —
/// a natively flung jar has always carried the right word.
///
/// The fix is the deletion: the generic seats already hold both words.
/// `MGC_NO_MC2_ARC_KEEPS_KILLER_LATCH=1` restores the clobber.
pub(crate) fn no_mc2_arc_keeps_killer_latch() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_ARC_KEEPS_KILLER_LATCH").is_some())
}

/// A/B toggle for the `subSpellIndex_0x2A_42` ABSENCE on the
/// `sub_6DCA0` BAND ARMS THEMSELVES. Every arm of the shared band
/// launcher stamps `a4x->subSpellIndex_2` onto the flyer it spawns
/// EXCEPT two: `a3 == 0` (fireball, EF:44417-30) and `a3 == 0xD`
/// (steal mana, EF:44447-56). BYTE-VERIFIED in the shipped
/// NETHERW.EXE: the `a3 == 0` arm at VA 0x6DD40-0x6DD79 is
/// `movb $0xa,0x43(%ebx)` + `movb $0x0/$0x4c,0x44(%ebx)` + `mov
/// $0x9,%edi` and holds NO `mov %ax,0x2a(%ebx)`, while the `a3 <= 9`
/// arm three blocks down does write it.
///
/// The absence is MASKED on the human/rival cast path, where
/// `sub_693F0` (EF:56199) post-writes the TOKEN's own `@0x2A` — so a
/// player's fireball legitimately carries the tier value. It is
/// UNMASKED on the two launchers that post-write nothing: the CASTLE
/// TURRET (EF:30292-93 writes only `id_0x1A_26` + `word_0x96_150`)
/// and the MAGIC MINE (EF:29999-30000, the same two). Their fireballs
/// carry `NewEvent_4A050`'s ctor default 100, not the row's 160/180.
///
/// `MGC_NO_MC2_BAND_2A_ABSENCE=1` restores the old unconditional
/// table write.
pub(crate) fn no_band_2a_absence() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_BAND_2A_ABSENCE").is_some())
}

/// The `sub_6DCA0` arms that DO write the tier's `subSpellIndex_2`
/// onto the flyer they spawn. Spell 0 (fireball) and spell 13 (steal
/// mana) are the two that do not.
pub(crate) fn mc2_band_arm_writes_2a(spell: usize) -> bool {
    !matches!(spell, 0 | 13) || no_band_2a_absence()
}

/// A/B toggle for the ROUND-98 EXTENSION of the launch aim point to
/// the **whole class-15 fire table** — the band arms (spells 0 and 7,
/// `sub_693F0`/`sub_6A5C0`) and the two direct arms 98-1 left out
/// (spell 9 `sub_6AB00`, spell 13 `sub_6B3E0`).
/// See [`mc2_launch_axis_reach`].
///
/// ⚠ **UN-PARKED AND WITNESSED, round 98 close.** It shipped parked for
/// about an hour because its dig measured only `mc2l6-rival-spells-galore`
/// **pairs 0..2000** and saw zero rows — a WINDOW artefact, not an absence.
/// Spells 0/7/9/13 are Fireball / Lightning / Meteor / Steal Mana, i.e. four
/// of the commonest offensive casts, and the MC2 spells-galore take exercises
/// all of them. Re-measured on `recordings/mc2l0-spells-galore.mgcr`, whole
/// take, one binary, `MGC_RAW_SHADOW=1` row-set diff:
/// **fixed 1,251 / introduced 0**, and *every* fixed row is
/// `(9,0) dest_x` / `dest_y` / `dest_z` — precisely the lane this stamps.
/// ⭐ **A "no exemplar in the corpus" verdict is only as wide as the window it
/// was measured on.** Before parking a law for want of a witness, re-measure
/// it on a take that exercises the mechanic — the galores exist for this.
pub(crate) fn no_launch_axis_band() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_LAUNCH_AXIS_BAND").is_some())
}

/// A/B toggle for the ROUND-104 EXTENSION of the launch aim point to
/// the **terrain-tail eight** — the class-15 fire handlers whose
/// `axis_0x9A_154x` step is `4096` at **pitch 0** and then
/// `z = getTerrainAlt(dest)` (spells 15/16/17/18/20/21/23/25).
/// `MGC_NO_MC2_LAUNCH_AXIS_GROUND=1` restores the old
/// "the helper cannot express a terrain lookup" exclusion on BOTH
/// the human and the rival column. See [`mc2_launch_axis_reach`].
pub(crate) fn no_launch_axis_ground() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_LAUNCH_AXIS_GROUND").is_some())
}

/// ⭐⭐⭐ A/B toggle for the FOURTH member of the
/// `subSpellIndex_0x2A_42` ABSENCE SET — the **STEAL MANA** bolt
/// `(9,8)` (spell 13). `MGC_NO_MC2_STEAL_2A_ABSENCE=1` restores the
/// old write on BOTH the human and the rival column.
///
/// The other three (possession basic/leveled, summon army) are
/// direct class-15 thunks with no `sub_6DCA0` at all. Steal Mana is
/// different and was missed because it *does* go through the band:
/// `sub_6B3E0` (EF:57545-57) calls `sub_6DCA0(caster, pos, 0xD, …)`
/// — but the band's `a3 <= 0xD` arm (EF:44460-68) is
/// ```c
///     v14x = _4A190(a2x, 9, 8);
///     if (v14x) { v14x->byte_0x43_67 = 10; v14x->byte_0x44_68 = 25; }
/// ```
/// and falls straight to `LABEL_60` — it is the ONE band arm that
/// writes neither `subSpellIndex_0x2A_42` nor `byte_0x46_70` (every
/// other arm reaches one or both, EF:44412/44455/44477/44494/44509/
/// 44521/44535). And `sub_6B3E0`'s own post-write list on the
/// returned bolt (EF:57563-77) is `word_0x26_38`, `id_0x1A_26`,
/// `position.z`, `mana_0x90_144`, `dword_0x10_16`, `byte_0x46_70`,
/// `axis_0x9A_154`, `yaw_0x1C_28`, `pitch_0x1E_30` — no `@0x2A`
/// either. So the bolt keeps `NewEvent_4A050`'s ctor **100**
/// (Events.cpp:569), and so does the `(10,25)` burst its impact
/// mints, because `sub_662E0`'s impact arm COPIES the bolt's word
/// into it.
///
/// ⚠ The payload is NOT the drain: the (10,25) burst's own tick
/// `sub_33E20` area-stamps ch3 with `byte_0x46_70` — the TIER index,
/// which `sub_6B3E0` does write — and `sub_61050` resolves
/// `SPELLS[13].subspell[tier]` itself ([`Gen::mc2_blast25_tick`]
/// reads `f71`, never `f44`). Nothing reads the copied word.
///
/// WITNESS (free-run raw shadow, mc2l6-rival-spells-galore):
/// `(9,8) f2a` 1,283 rows over 41 slots t=8019..19464, retail **100**
/// / port 10, and its sibling `(10,25) f2a` 174 rows over 22 slots,
/// retail **100** / port 4000.
pub(crate) fn no_steal_2a_absence() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_STEAL_2A_ABSENCE").is_some())
}

/// A/B toggle for KEYING THE POSSESSION BOLT'S AIM REACH ON THE
/// SPAWNED SUBTYPE: set `MGC_NO_MC2_POSSES_AXIS_REACH_BY_SUBTYPE` to
/// restore the pre-dig [`mc2_launch_axis_reach`], which picked spell
/// 1's reach off the SUBSPELL ROW's `life_0x1A`.
///
/// ⭐⭐⭐ THE HELD RE-FIRE IS A BASIC BOLT CARRYING A LEVELED ROW.
/// `sub_68DE0`'s re-press block (EF:55995-56010) calls **`sub_69900`**
/// — the BASIC (9,1) spawner — while `byte_0x46_70` still names tier 1
/// or 2, and the port's [`World::mc2_possess_launch`] reproduces that
/// (it is handed `subtype = 1` with the held tier's `sub`). But the
/// axis stamp keyed on `sub.life`, so those bolts stepped
/// `sub_69640`'s **0x4000** instead of `sub_69900`'s **10240**.
/// BYTE-VERIFIED: `sub_69900` in the shipped `NETHERW.EXE` pushes its
/// reach at file **0x8E1D6** — `68 00 28 00 00  push $0x2800` = 10240
/// — from `lea 0x9a(%ebx),%eax` at 0x8E1F7, with the bearing pair
/// `0x1c(%edx) + 0x18(%eax)` / `0x1e(%edx) + 0x1a(%eax)` (the caster's
/// yaw/pitch plus the wizext aim offsets) at 0x8E1CE-0x8E1F3.
///
/// ⚠ THE BRIEF'S HAND-OFFSET HYPOTHESIS IS REFUTED. mc2l11 t=42587
/// slot 287 is the census head for `(9,1) dest_*`: the bolt records
/// yaw **306** / pitch **112** and its caster (slot 100, the human)
/// stands at (767, 45337, 3150) with yaw **306** / pitch **112** —
/// the wizext `@0x18`/`@0x1A` offsets are **0**, and `p.heading` /
/// `p.pitch` were already exactly retail's terms. Retail's recorded
/// dest (8545, 39642, −299) is that origin stepped **10240** at that
/// bearing to the unit (horizontal leg 9,638 = 10240·cos(112/2048·2π),
/// drop −3,449); the port's (13213, 36224, −2369) is the SAME origin
/// and the SAME bearing stepped **16384** (leg 15,424). One constant,
/// picked off the wrong field.
///
/// WITNESS (free run, `MGC_RAW_SHADOW=1`): `(9,1) dest_x/dest_y/
/// dest_z` ≈ 30,200 rows over 6 takes — mc2l11 255 rows each over 12
/// slots (t=42587..43913) and mc2l6-rival-spells-galore 24 each over 3
/// slots (t=3993..4022, the take whose held-tier-2 re-fire is already
/// documented at the `byte_0x3C_60` arm in
/// [`World::mc2_manifestation_tick`]).
pub(crate) fn no_mc2_posses_axis_reach_by_subtype() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_POSSES_AXIS_REACH_BY_SUBTYPE").is_some())
}

/// A/B toggle for the LEVELED POSSESSION bolt's `dword_0x10_16`
/// SQUARED-REACH home (`MGC_NO_MC2_POSSES_REACH_ROOT=1` restores the
/// old i16-truncated square, i.e. 0). Full citation at the human seat
/// in [`World::mc2_cast_posses`]; the rival funnel shares the switch
/// because `sub_69640` is caster-generic.
pub(crate) fn no_posses_reach_root() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_POSSES_REACH_ROOT").is_some())
}

/// A/B toggle for the RIVAL column's `subSpellIndex_0x2A_42` ABSENCE
/// (`MGC_NO_RIVAL_LAUNCH_2A_ABSENCE=1` reverts). Separate from
/// [`no_launch_2a_absence`] so the twin call paths stay independently
/// attributable. See `mc2/rivals.rs` `mc2_rival_emit`.
pub(crate) fn no_rival_launch_2a_absence() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_LAUNCH_2A_ABSENCE").is_some())
}

/// A/B toggle for the RIVAL column's LAUNCH AIM POINT
/// (`MGC_NO_RIVAL_LAUNCH_AXIS=1` reverts): `mc2/rivals.rs`
/// `mc2_rival_emit` is the twin call path of [`World::mc2_launch`] —
/// the class-15 fire handlers are caster-generic, so the rival funnel
/// takes the same `axis_0x9A_154x` stamp. See
/// [`mc2_launch_axis_reach`] for the table and its citations.
pub(crate) fn no_rival_launch_axis() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_RIVAL_LAUNCH_AXIS").is_some())
}

/// A/B toggle for the CLASS-9 BIRTH `roll`/`fov` ABSENCE
/// (`MGC_NO_MC2_LAUNCH_ROLL_ABSENCE=1` restores the old birth stamp).
/// See [`mc2_launch_axis_reach`]'s sibling note and
/// [`super::cast::World::mc2_launch`].
pub(crate) fn no_launch_roll_absence() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_LAUNCH_ROLL_ABSENCE").is_some())
}

/// ⭐⭐⭐ A FIELD-HOMING BUG, AND THE DECOMPILE'S OWN NAME IS WHAT
/// CAUSED IT. The Lightning T3 twins cross-link through retail's
/// `word_0x34_52` — **offset 0x34**, whose port home is
/// `Ent::f54` (`import_ent_mc2`: `f54: tr(r.f34)`;
/// `port_ent_lanes_mc2` publishes `e.f54` in the `f34` lane, and
/// `mc2/mobs.rs`'s own field table already says
/// "`word_0x34_52` subentity chain→f54"). Both twin-fan sites wrote
/// **`f52`** instead — the port's home for `word_0x32_50`, @0x32 —
/// because the remc2 name's decimal suffix (`_52`) reads exactly like
/// a port field name. The port's `fNN` are MC1-record decimal
/// offsets; remc2's `_NN` suffix is the MC2 decimal offset. They
/// collide on this one word and nowhere else in the pair.
///
/// CITATION. `sub_6A5C0` (banner `//----- (0006A5C0)`, EF:57002-06):
/// ```text
///   if (v13x) {
///       v12x->word_0x34_52 = v13x - D41A0_0.struct_0x6E8E;
///       v13x->word_0x34_52 = v12x - D41A0_0.struct_0x6E8E;
///   }
/// ```
/// BYTE-VERIFIED in the shipped `NETHERW.EXE` (file = VA + 0x24800,
/// so `sub_6A5C0` = file 0x8EDC0); the cross-link pair is the only
/// `0x34` store in the body, at file 0x8F158-0x8F16E:
/// ```text
///   8f158  8b 55 e0        mov    -0x20(%ebp),%edx   ; v12x
///   8f15b  66 89 42 34     mov    %ax,0x34(%edx)     ; = index(v13x)
///   8f15f  29 ca           sub    %ecx,%edx
///   8f161  89 d0 / c1 fa 1f / f7 fe                  ; ptr -> index
///   8f168  8b 55 e4        mov    -0x1c(%ebp),%edx   ; v13x
///   8f16b  66 89 42 34     mov    %ax,0x34(%edx)     ; = index(v12x)
/// ```
/// The lane's retail CONSUMER is `sub_66FD0`'s (action 12) beacon
/// arm, which reads `v4 = a1x->word_0x34_52`, despawns
/// `Entities_EA3E4[v4]` through `sub_57F20` and clears the word
/// (banner `00066FD0`, resolve by address) — PORTED in round 150 as
/// `Gen::mc2_m12_beacon_boost` (`MGC_NO_MC2_M12_BEACON_BOOST`, proj.rs),
/// and it reads @0x34, which is why the link lives there.
///
/// WITNESS (`MGC_RAW_SHADOW=1`, the ungraded lane census): 177 `f32`
/// rows and 177 `f34` rows on (9,12) records, one pair per twin, with
/// the SAME value on both sides — mc2l22 106 rows t=4637..20680 over
/// 23 slots (t=4637 slot 853: retail f32 0 / f34 901, port f32 901 /
/// f34 0), mc2l24 23, mc2l6-rival-spells-galore 36, mc2l0-spells-galore
/// 12. Both call paths carried it: the human fan in
/// `World::mc2_spell_fire` and the rival funnel's
/// `mc2_rival_emit` (`mc2/rivals.rs`).
///
/// `MGC_NO_MC2_M12_HANDLE_HOME=1` restores the `f52` home for A/B.
pub(crate) fn no_mc2_m12_handle_home() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_M12_HANDLE_HOME").is_some())
}

/// ⭐⭐⭐ THE SAME `roll`/`fov` BIRTH ABSENCE, ON THE **NON-CAST**
/// CLASS-9 SPAWNERS — the world's own emitters, which are not in
/// [`no_launch_roll_absence`]'s class-15 table at all. Two sites, two
/// retail functions, one law: a freshly minted flyer leaves
/// `NewEvent_4A050`'s memset **0** in `roll_0x20_32` / `fov_0x22_34`
/// and only the NEXT tick's `mc2_flyer_tick` one-shot snapshots them
/// off `yaw`/`pitch`, so the divergence is exactly ONE row per bolt,
/// on its birth tick, in the ungraded `roll` lane.
///
/// **1. The LIGHTNING-STORM cloud's rained beams** — `sub_35640`
/// (EF:25898, the `(10,38)` action-40 tick, [`Gen::mc2_storm_tick`]).
/// Its per-beam store list in the shipped `NETHERW.EXE` (file
/// 0x59F37-0x59F74, the `if (v8x)` arm) is complete and short:
/// ```text
///   59f37  66 8b 43 1a        mov  ax,[ebx+0x1a]     ; owner
///   59f3b  66 89 41 1a        mov  [ecx+0x1a],ax
///   59f4e  89 41 08           mov  [ecx+0x8],eax     ; life /= 3
///   59f51  66 8b 43 1c        mov  ax,[ebx+0x1c]     ; yaw  <- cloud yaw
///   59f55  66 89 41 1c        mov  [ecx+0x1c],ax
///   59f59  66 8b 43 1e        mov  ax,[ebx+0x1e]     ; pitch<- cloud pitch (56)
///   59f5d  66 89 41 1e        mov  [ecx+0x1e],ax
///   59f65  c6 41 43 0a        movb [ecx+0x43],0xa    ; impact (10,23)
///   59f69  c6 41 44 17        movb [ecx+0x44],0x17
///   59f70  66 89 41 2a        mov  [ecx+0x2a],ax     ; subSpell
/// ```
/// There is no `66 89 41 20` and no `66 89 41 22` anywhere in the
/// function. The port stamped both.
///
/// **2. The GROUND-VORTEX's upward `(9,0)`** — `sub_32A70` (EF:23928,
/// the `(10,18)` action-18 tick, [`Gen::mc2_summit18_tick`]). Same
/// shape, shipped bytes at file 0x5746B-0x57489:
/// ```text
///   5746b  66 8b 43 1c        mov  ax,[ebx+0x1c]
///   5746f  66 c7 42 1e 7e fe  movw [edx+0x1e],0xfe7e ; pitch = -386
///   57475  c6 42 43 0a        movb [edx+0x43],0xa    ; impact (10,17)
///   57479  c6 42 44 11        movb [edx+0x44],0x11
///   57489  66 89 42 1c        mov  [edx+0x1c],ax     ; yaw (HIBYTE &= 7)
/// ```
/// then the `@0x9A` aim vector — again no `0x20(%edx)` / `0x22(%edx)`.
///
/// `MGC_NO_MC2_SPAWN_ROLL_ABSENCE=1` restores the pre-dig birth stamp
/// on both. (Round 148, dig w148t. Corpus: `(9,9) roll` 6,218 rows
/// over 23 takes is the storm's rain; `(9,0) roll` 107 rows over 18
/// takes, port value 1280 = the vortex's own `yaw += 1280` pulse, is
/// the ground vortex.)
pub(crate) fn no_spawn_roll_absence() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SPAWN_ROLL_ABSENCE").is_some())
}

/// A/B toggle for the MAGIC-MINE CARRIER's absent TIER BYTE: set
/// `MGC_NO_MC2_MINE_CARRIER_TIER_BYTE` to restore the pre-dig
/// behaviour, where [`World::mc2_launch`]'s generic `arm.charge` leg
/// stamped the tier's `life` into the `(9,29)` carrier's
/// `byte_0x46_70` (our `f71`).
///
/// ⭐ AN ABSENCE IN AN ENUMERATED LIST. The mine's own fire handler
/// `sub_6CAC0` (EF:58320-58381) writes exactly ten words on the
/// carrier it mints — `actSpeed`, `byte_0x43_67`, `byte_0x44_68`,
/// `word_0x26_38`, `id_0x1A_26`, `position.z`, `mana_0x90_144`,
/// `subSpellIndex_0x2A_42`, `dword_0x10_16`, `axis_0x9A_154x`,
/// `yaw_0x1C_28`, `pitch_0x1E_30` — and `byte_0x46_70` is NOT one of
/// them. It only ever READS the CASTER's tier byte
/// (`v2x->subSpellIndex_0x2A_42 = a1x->byte_0x46_70;`), which in the
/// shipped `NETHERW.EXE` is the single `66 0f be 40 46  movsbw
/// ax,[eax+0x46]` at file 0x913B4 — a load, and the function's ONLY
/// reference to @0x46. Nothing reads the carrier's own @0x46 either:
/// `sub_67960` (its tick) never touches it, and the `(10,78)` mine it
/// lands re-indexes the spell row off `subSpellIndex` instead.
/// WITNESS: mc2l6-rsg `(9,29) b46` 94 rows over 11 slots
/// (t=13304..23651, retail 0 / port 1); mc2l4 11 rows (port 2).
pub(crate) fn no_mine_carrier_tier_byte() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_MINE_CARRIER_TIER_BYTE").is_some())
}

/// ⭐⭐⭐ THE LAUNCH AIM POINT IS A WHOLE-TABLE LAW, NOT A FOUR-ARM
/// ONE. Every class-15 spell "fire" handler in the shipped MC2 binary
/// copies the CASTER's own `position_0x4C_76` into the flyer's
/// `axis_0x9A_154x` (@0x9A) and steps it along the launch bearing with
/// `MoveEntity_57FA0`. Enumerating every `axis_0x9A_154x =` in the
/// class-15 band of `EventsFunctions.cpp` and pairing each with its
/// `Events.cpp` action-dispatch group (five handlers per spell, the
/// last one being the fire) gives the COMPLETE table — twenty sites,
/// one per spell, with exactly ONE spell missing (Fool's Mana):
///
/// | spell | handler | EF | reach | pitch |
/// |---|---|---|---|---|
/// | 0 fireball   | `sub_693F0` | 55871 | 0x4000 | caster |
/// | 1 possess L0 | `sub_69900` | 56059 | 10240  | caster |
/// | 1 possess L1+| `sub_69640` | 55976 | 0x4000 | caster |
/// | 2 castle     | `sub_69AB0` | 56140 | 4096   | 0 + terrain alt |
/// | 7 lightning  | `sub_6A5C0` | 56622/56675 | 0x4000 | caster |
/// | 9            | `sub_6AB00` | 56816 | 10240  | caster |
/// | 13 steal     | `sub_6B3E0` | 57214 | 0x4000 | caster |
/// | 14 duel      | `sub_6B610` | 57310 | 10240  | caster |
/// | 15/16/17/18/20/21/25 | `sub_6B870`/`6BAB0`/`6BCF0`/`6BF30`/`6C3E0`/`6C620`/`6CFA0` | 57382/57455/57527/57597/57750/57827/58161 | 4096 | 0 + terrain alt |
/// | 19 army      | `sub_6C170` | 57673 | 0x4000 | caster |
/// | 22 fool's    | `sub_6C870` | —     | NONE   | — |
/// | 23 mine      | `sub_6CAC0` | 57996 | 4096   | 0 + terrain alt |
/// | 24 alliance  | `sub_6CD20` | 58077 | 0x4000 | caster |
///
/// ⭐ VERIFIED IN THE SHIPPED `NETHERW.EXE`, not just the decompile:
/// `sub_693F0` at file **0x8DBF0** ends its spawn tail with
/// `0x8DD47 lea 0x9a(%edi),%edi` / `0x8DD4D lea 0x4c(%ebx),%esi` /
/// `movsl ; movsw` (the six-byte `axis = caster.position` copy) then
/// `0x8DD61 push $0x4000` with pitch `0x1e(%ebx)+wizext@0x1a` and yaw
/// `0x1c(%ebx)+wizext@0x18` into `call 0x7c7a0` (= `MoveEntity_57FA0`,
/// linear 0x57FA0). `sub_69900` at file **0x8E100** carries the same
/// pair at `0x8E1B3`/`0x8E1B9` with `0x8E1D6 push $0x2800` = **10240**.
///
/// This helper returns `(reach, use_pitch)` for the arms that end in a
/// plain polar step. The seven `4096 / pitch 0 / z = getTerrainAlt`
/// arms and the castle ball are DELIBERATELY EXCLUDED: their tail is
/// a terrain lookup this helper cannot express, and the port already
/// uses `dest_x`/`dest_y`/`site_z` on the (9,10) castle ball as its
/// own flight target (`mc2/rivals.rs` `e.dest_x = tx << 8`), so a
/// generic stamp there would clobber a landed lane.
///
/// ⭐⭐⭐ ROUND 104 — **THAT EXCLUSION WAS THE LAST HOLE, AND IT WAS
/// EIGHT SPELLS WIDE.** The helper CAN express the terrain tail: it
/// is a third flag, not a different shape. All eight sites are the
/// same three statements, byte for byte:
///
/// ```text
///   v->axis_0x9A_154x = caster->position_0x4C_76;
///   MoveEntity_57FA0(&v->axis_0x9A_154x,
///                    caster->wizext->nextEntity_0x18_24
///                        + caster->yaw_0x1C_28, 0, 4096);
///   v->axis_0x9A_154x.z = getTerrainAlt_10C40(&v->axis_0x9A_154x);
/// ```
///
/// EF:57382-83 (15) · 57455-56 (16) · 57527-28 (17) · 57597-98 (18) ·
/// 57750-51 (**20**) · 57827-28 (21) · 57996-97 (23, the mine) ·
/// 58161-62 (25). The handler-to-spell map is the address ladder
/// itself — the class-15 dispatch groups run five entries per spell
/// in ascending order (`Events.cpp:3609-3800`), and the anchors
/// `sub_6B610` = 14 (duel), `sub_6C170` = 19 (army), `sub_6C870` = 22
/// (Fool's Mana), `sub_6CD20` = 24 (alliance) pin it with no slack:
/// `6B870`=15 `6BAB0`=16 `6BCF0`=17 `6BF30`=18 `6C3E0`=20 `6C620`=21
/// `6CAC0`=23 `6CFA0`=25. `sub_6C3E0`'s own body confirms it — its
/// `sub_6DCA0` call passes the band index **`0x14u` = 20**
/// (EF:57739).
///
/// The **castle ball (spell 2, `sub_69AB0` EF:56140-45) stays
/// excluded** and is the ONE member of the family whose tail differs:
/// it lands the terrain read in a temporary, writes `byte_0x43_67`/
/// `byte_0x44_68` between, and — decisively — the whole block is the
/// `CastleEntityIndex <= Entities[0]` (castle-LESS) arm, the `else`
/// of which stamps `word_0x96_150` and no axis at all. The port's
/// castle ball already drives its flight off `dest_x`/`site_z`.
///
/// WITNESS: mc2l22 t=63319, the take's LAST head. The human casts
/// GRAVITY WELL (spell 20) from (19801, 50761, 4724) at yaw 75;
/// retail records the `(9,22)` at slot 816 with
/// `dest (20735, 46773, 2324)` — horizontal leg exactly **4096** at
/// **pitch 0**, and `2324` is `getTerrainAlt` at that point, not any
/// polar z. The port left all three at zero.
///
/// `MGC_NO_MC2_LAUNCH_AXIS_GROUND=1` reverts exactly this extension
/// (both columns), leaving the pitch-carrying arms untouched.
///
/// ⭐⭐⭐ THE POSSESSION PAIR'S REACH IS THE **SPAWNED SUBTYPE'S**, NOT
/// THE TIER ROW'S. `subtype` is the class-9 model the arm actually
/// mints — the caller has it, and for spell 1 it is the only honest
/// discriminator, because the HELD RE-FIRE reaches
/// [`World::mc2_possess_launch`] with `subtype = 1` while still
/// carrying the held TIER's subspell row (`life` 1 or 2). Retail's
/// re-fire is a direct `sub_69900(a1x, v1x)` call out of `sub_68DE0`
/// (EF:56004) — the BASIC spawner, `push $0x2800` = **10240** at
/// `NETHERW.EXE` file **0x8E1D6** — so a held Mana Magnet's plain
/// claim bolts take the basic reach, where keying on `life` handed
/// them the leveled `sub_69640`'s 0x4000. See
/// [`no_mc2_posses_axis_reach_by_subtype`].
///
/// Returns `(reach, use_pitch, ground_snap)`.
pub(crate) fn mc2_launch_axis_reach(
    spell: usize,
    life: i8,
    subtype: u8,
) -> Option<(i16, bool, bool)> {
    match spell {
        // sub_693F0 EF:55871 / sub_6A5C0 EF:56622,56675 — the band's
        // own fire blocks, both 0x4000 at the caster's live pitch.
        0 | 7 => Some((0x4000, true, false)),
        // sub_69640 EF:55976 (leveled) vs sub_69900 EF:56059 (basic):
        // the possession pair splits on WHICH SPAWNER RAN, and so does
        // its reach. The spawner is named by the class-9 model it
        // mints: 1 = `sub_69900`, 17 = `sub_69640`'s inline block.
        1 if !no_mc2_posses_axis_reach_by_subtype() => {
            Some((if subtype == 1 { 10240 } else { 0x4000 }, true, false))
        }
        1 if life == 0 => Some((10240, true, false)),
        1 => Some((0x4000, true, false)),
        9 => Some((10240, true, false)),        // sub_6AB00 EF:56816
        13 => Some((0x4000, true, false)),      // sub_6B3E0 EF:57214
        19 | 24 => Some((0x4000, true, false)), // sub_6C170 / sub_6CD20
        // ⭐ THE TERRAIN-TAIL EIGHT — reach 4096 at pitch 0, then
        // `z = getTerrainAlt(dest)`. sub_6B870 / sub_6BAB0 /
        // sub_6BCF0 / sub_6BF30 / sub_6C3E0 / sub_6C620 / sub_6CAC0 /
        // sub_6CFA0, EF:57382 / 57455 / 57527 / 57597 / 57750 /
        // 57827 / 57996 / 58161.
        15 | 16 | 17 | 18 | 20 | 21 | 23 | 25 if !no_launch_axis_ground() => {
            Some((4096, false, true))
        }
        _ => None,
    }
}

/// ⭐⭐⭐ A/B toggle for the MC2 HUMAN shield's **ARMED** stage — the
/// fourth and last wizard-buff column to get the two-stage absorb.
///
/// `sub_6A480` (EF:56496-541) keys on the TIER's `life_0x1A`, not on
/// the tier index: `life == 0` re-stamps `parent.byte[1] |= 0x40`
/// (CHARGED) on every afforded tick, `life == 1` stamps
/// `parent.byte[2] |= 0x40` (ARMED) on the FIRST tick only, and a
/// tier declaring anything else stamps NEITHER. SPELLS.DAT gives
/// spell 6 `life_0x1A` = 0/0/1, so Shield III is the ARMED stage.
///
/// `sub_5EFA0`'s absorb (EF:60676-93) opens on the VICTIM's own flag
/// word — `if (byte[1] & 0x40 || byte[2] & 0x40)` — and then splits:
///
/// ```text
///   byte[1] & 0x40  (CHARGED): v10 = dword_0x5E_94 / 4;
///                              mana -= v10; dword_0x5E_94 = v10;
///                              byte[1] &= 0xBF;              // spent
///   else            (ARMED):   dword &= 0xFFBFBFFF;          // both off
///                              dword_0x5E_94 = 0;            // hit NULLED
///                              byte[1] |= 0x40;              // -> CHARGED
/// ```
///
/// The port's human column had only the CHARGED arm and no ARMED
/// lane at all (`Player` carried one `shield` bool), so a human
/// Shield III quartered its first hit where retail nulls it outright
/// and then quarters the NEXT one.
///
/// Set `MGC_NO_MC2_SHIELD_ARMED` to restore the single-stage
/// behaviour (the cast latches `player.shield` for every tier and the
/// import seat ignores `byte[2] & 0x40`).
pub(crate) fn no_mc2_shield_armed() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SHIELD_ARMED").is_some())
}

/// ⭐⭐⭐ **THE SPEED TOKEN STEERS BY THE *COMMAND* REGISTER, NOT BY
/// THE CARPET'S ACTUAL SPEED — AND THE TWO PART EVERY TIME THE SPELL
/// IS CAST WHILE STILL COASTING BACKWARD.**
///
/// `GetScroll_69DB0` (`reference/remc2/remc2/engine/EventsFunctions.cpp`
/// — IDA banner `//----- (00069DB0)`, signature the next line) opens
/// its body with
///
/// ```text
///   if (v1x->dword_0xA4_164x->speed_0xc_12 >= 0) v2 = 1; else v2 = -1;
///   ...
///   v1x->dword_0xA4_164x->speed_0xc_12 = v2 * minSpeed * (sub + 1);   // arm tick
///   v1x->actSpeed_0x82_130            = v1x->dword_0xA4_164x->speed_0xc_12;
/// ```
///
/// `dword_0xA4_164` is the caster's `Type_str_164` flight block and
/// `speed_0xc_12` is its **command** word — the pose channel's
/// `tgt_speed` / `RetailPlayerMc2::cmd_speed` (+998+12) — NOT the
/// entity's `actSpeed_0x82_130` (+0x82), which the token writes a
/// line later FROM it. Shipped `NETHERW.EXE`: the compare is at file
/// `0x8E603` (`mov eax,[esi+0xA4]` / `cmp word [eax+0xC],0` /
/// `jl`), i.e. VA `0x69E03` = `0x8E603 − 0x24800`, and the pair of
/// stores is `mov [eax+0xC],dx` + `mov [esi+0x82],dx`.
///
/// The port derived the sign from the POSE's `speed`, which is
/// `actSpeed`. `mc2/cast.rs` even wrote the discrepancy down and
/// dismissed it — *"the two agree on every tick of a live window and
/// on the arm tick they differ only while a decelerating carpet
/// crosses zero"* — and that dismissal is exactly the head:
/// **mc2l33 t=8896**. The player had cast Speed BACKWARD at t=8891
/// (`cmd_speed`/`actSpeed` −240 → −160), walled into a cave at 8892,
/// which zeroed `cmd_speed`, and by 8895 the carpet was still coasting
/// at `actSpeed` −112 with `cmd_speed` **0**. Re-pressing Speed at
/// 8896 makes retail read `0 >= 0` → forward → `+240`; the port read
/// −112 → backward → `−240`, flew into the wall behind it, took the
/// refusal's `tgt_speed = 0` + `waterCounter++`, and never moved
/// again. One bit, six pose lanes.
///
/// Set `MGC_NO_MC2_SPEED_SIGN_CMD` to restore the `actSpeed` read.
pub(crate) fn no_mc2_speed_sign_cmd() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SPEED_SIGN_CMD").is_some())
}

/// ⭐⭐⭐ **SPEED IS THE ONE MANIFESTATION WHOSE *UNAFFORDABLE* TICK
/// DOES NOT COLLAPSE ITS WINDOW — AND A DYING WIZARD IS
/// UNAFFORDABLE, SO EVERY DEATH FALL USED TO FIRE THE ±80 RESTORE.**
///
/// The twenty-five other MC2 handlers close their effect body with a
/// bare `else { word_0x2E_46 = 1 }` (EF:56236, :56372, :56700,
/// :56819, :57041, :57107, :57388, :57586, :58049 …), and the port
/// implements that as one shared arm. `GetScroll_69DB0`
/// (`reference/remc2/remc2/engine/EventsFunctions.cpp`, IDA banner
/// `//----- (00069DB0)`; the arm is at :56567-70) is the EXCEPTION,
/// and it is the ONLY function in the binary that reads
/// `word_0xe_14` at all:
///
/// ```text
///   if (!sub_68D50(a1x, v1x) || v1x->dword_0xA4_164x->word_0xe_14)
///   {
///       if (v1x->dword_0xA4_164x->word_0xe_14)
///           a1x->word_0x2E_46 = 1;          // the BRAKE alone
///   }
///   else { ...the effect body... }
///   v8 = a1x->word_0x2E_46 - 1;             // shared by both arms
///   a1x->word_0x2E_46 = v8;
///   if (!v8) { speed = minSpeed * v2; actSpeed = speed; sub_6D880(a1x); }
/// ```
///
/// Shipped `NETHERW.EXE`, VA `0x69F79` = file `0x8E779` (+0x24800) —
/// the join of the afford `je` at `0x8E61F` (on `sub_68D50`'s return,
/// `call 0x8D550` at `0x8E615`) and the brake `jne` at `0x8E630`:
///
/// ```text
///   8e779: 8b 86 a4 00 00 00   mov  0xa4(%esi),%eax   ; caster Type_str_164
///   8e77f: 66 83 78 0e 00      cmpw $0x0,0xe(%eax)    ; word_0xe_14
///   8e784: 74 06               je   0x8e78c           ; ⭐ NO brake ⇒ NO collapse
///   8e786: 66 c7 43 2e 01 00   movw $0x1,0x2e(%ebx)   ; brake ⇒ word_0x2E_46 = 1
///   8e78c: 66 8b 4b 2e         mov  0x2e(%ebx),%cx    ; the plain decrement
///   8e790: 66 49               dec  %cx
///   8e792: 66 89 4b 2e         mov  %cx,0x2e(%ebx)
///   8e796: 75 35               jne  0x8e7cd           ; nonzero ⇒ no restore
///   8e798: 66 0f af be 84 …    imul 0x84(%esi),%di    ; ±minSpeed = the ±80 restore
/// ```
///
/// And the afford test `sub_68D50` (banner `//----- (00068D50)`,
/// :55901-03) bails on the CASTER's own vitals —
/// `if (locEvent2->mana_0x90_144 < 0) return false;` then
/// `if (locEvent2->life_0x8 < 0) return false;` — so a wizard who has
/// just been killed is unaffordable for every remaining tick of his
/// death fall, while his Speed window keeps counting down untouched.
///
/// **mc2l19-taketwo t=3257 is the witness.** The player is flying
/// backward under Speed (`cmd_speed`/`actSpeed` −240 at t=3243, −160
/// from t=3244), takes lethal damage at t=3256 (`life` −1200,
/// `actionIndex` 0 → 2) and touches down at t=3257. Retail's Speed
/// token (slot 16, the `(15,3)`) records `word_0x2E_46` **287 → 286**
/// across that boundary — a plain decrement, no collapse — and the
/// flight columns stay pinned at **−160** through the landing, the
/// whole dead wait and past it (t=3258…3320 all −160). The port read
/// `mc2_afford` false, collapsed the counter to 1, decremented it to
/// 0 in the same tick and mailed `pending_speed_base = −80`, so the
/// carpet's own dispatch slammed `act_speed`/`tgt_speed` to −80 and
/// the corpse fell 79 units short. The head is not pose-only: the
/// touchdown's 26-token scatter and the `(10,40)` grave are placed
/// from the carpet's mid-tick position, so slot 6 and the whole
/// class-15 band land at the wrong spot too.
///
/// Set `MGC_NO_MC2_SPEED_BROKE_SURVIVES` to restore the shared
/// collapse on Speed's unaffordable tick.
pub(crate) fn no_mc2_speed_broke_survives() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SPEED_BROKE_SURVIVES").is_some())
}

/// ⭐⭐⭐ **SHIELD III DECREMENTS ITS BURST COUNTER *BEFORE* IT BILLS,
/// SO IT NEVER PAYS — AND NEVER PINS THE REGEN ON ITS LAST TICK.**
///
/// `sub_6A480`'s two arms are the same four statements in a DIFFERENT
/// ORDER, and the order is the whole law (EF:56513-15 vs 56525-28; shipped
/// `NETHERW.EXE` @ file `0x8ECDD` / `0x8ED0B`, `off = 0x34800 +
/// linear − 0x10000`):
///
/// ```text
///   life_0x1A == 0 :  byte[1] |= 0x40 ; sub_68DE0 ; word_0x2E_46--
///                     8ecf5 mov %ah,0xd(%esi) / 8ecf8 call 0x8d5e0 / 8ecfd dec
///   life_0x1A == 1 :  [if 46==48] byte[2] |= 0x40 ; word_0x2E_46-- ; sub_68DE0
///                     8ed25 orb $0x40,0xe(%esi) / 8ed29..8ed30 dec / 8ed34 call 0x8d5e0
/// ```
///
/// `sub_68DE0` (EF:55569) keys its FULL-COST debit on
/// `word_0x2E_46 == word_0x30_48`. In the tier-2 arm that compare runs
/// on the ALREADY-DECREMENTED counter, so it is false by construction:
/// **Shield III's 10,000 is never charged.** It falls into the else —
/// `if (v2 && manaRegen > 0) manaRegen = 0` — which pins the regen on
/// the ARM tick (where the tier-0/1 arm would have parked `-maxMana`),
/// and, because `v2` is the post-decrement counter, does NOT pin on
/// the tick the window ENDS on.
///
/// SPELLS.DAT gives spell 6 `life_0x1A` = 0 / 0 / 1, so this is
/// Shield III alone. Witness — mc2l6-rival-spells-galore, all three of
/// its `sel[6] == 2` windows:
/// - t=22531 is the arm (`f2e` 301 → **300**, `byte[2] |= 0x40`
///   visible as the carpet's `flags` 269 → 4194573): retail's `d88`
///   goes 739 → **0** and `mana` 1474488 → 1475227 → 1475227 → …,
///   FLAT. The port stamped `d88 = −10000` and landed on 1465227 —
///   the recorded −10,000 wall at t=22532, repeated at t=22731 and
///   t=23117.
/// - t=22721 is the re-press CANCEL (`move_bits` 0 → 16,
///   `sub_5F660` case 0xE pins `f2e` to 1, `f2e` 111 → **0**): retail
///   takes the decrement first, `sub_68DE0` sees `v2 == 0`, does NOT
///   pin, and the wizard's freshly recomputed `d88` 739 stands —
///   `mana` 1475165 → 1475904 at t=22722. The port pinned and held,
///   which is exactly the recorded −739 wall at t=22722.
///
/// ⚠ FREE-RUN-ONLY LANE: `manaRegen_0x88_136` is not in `EntObsMc2`,
/// and the pair importer (`mc2_applied_mana_delta`) reconstructs the
/// delta from retail's OWN post-tick `f2e`/`f30` — where the arm tick
/// already reads `300 != 301` (mid-burst pin) and the last tick reads
/// `0` (skipped). ⚠⚠ **THAT CLAIM WAS WRONG AND A COMMENT SAYING A
/// LANE IS SWEPT IS A DIG LEAD** (round 126 dig I): the import seat is
/// `mc2_applied_mana_delta`, NOT the `@0x88` seat, and it re-derived
/// the pin from the RECORDED `@0x2E` without the pre-decrement — so
/// three mc2l24 pairs DO assert it (t=36824 / 40238 / 40567, token
/// slot 40 at `@0x2E == 1`). Both call paths now carry it; the unit
/// tests are `tests/mc2_shield_billing.rs` and
/// `a_refused_token_tick_and_shield_iii_leave_the_wizard_regen_alone`.
///
/// `MGC_NO_MC2_SHIELD3_PREDECREMENT=1` restores the shared skeleton
/// (bill on the arm tick, pin on every later tick).
pub(crate) fn no_mc2_shield3_predecrement() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SHIELD3_PREDECREMENT").is_some())
}

/// ⭐⭐ **THE DEATH SCATTER LEAVES `word_0x2E_46` STANDING.** The
/// port's [`World::mc2_scatter_spells`] zeroed the scattered
/// manifestation's window counter "so no effect body ever runs
/// again". Retail does not: `sub_5E310`'s 26-token loop
/// (EF:60533-56) is exactly
/// ```text
///   SpellEnabled[i] = 1;                 // the boolean "still known"
///   v19x->byte[0] &= 0xFE;               // out of the book
///   v19x->actionIndex_0x45_69++;         // 3M -> 3M+1 (the pickup state)
///   …the ±256 scatter and CopyEntityPosition…
///   v26x->life_0x8 = rand % 0x5A + 200;
/// ```
/// and the shipped bytes agree — `NETHERW.EXE` linear 0x5E310 → file
/// 0x82B10, the loop body at 0x82CFF
/// `andb $0xfe,0xc(%edx) / incb 0x45(%edx)` through 0x82DAB
/// `mov %edx,0x8(%esi)` (the life store) and 0x82DB9/0x82DBC
/// `incl -0x8(%ebp) / cmpl $0x1a,-0x8(%ebp)`: **there is no store to
/// `+0x2e` anywhere in `sub_5E310`.** The counter simply FREEZES at
/// whatever the last live tick left, because the record now dispatches
/// the pickup state and `GetScroll_69DB0`'s
/// `word_0x2E_46 > 0` body — with its trailing `dec` (file 0x8E78C
/// `mov 0x2e(%ebx),%cx / dec %cx / mov %cx,0x2e(%ebx)`) — is no longer
/// reached. It never counts down again: the scroll just expires on
/// `life`.
///
/// The port's own effect body was ALREADY unreachable without the
/// clear — `World::mc2_cast_tick` walks the BOOK (the scatter writes
/// the boolean 1 there) and demands `tick70 == 3 * spell`, which the
/// scatter has just bumped to `3 * spell + 1`.
///
/// WITNESSES — free-run `MGC_RAW_SHADOW=1`, lane `(15,3) f2e`, one
/// window per take, each of them a HUMAN DEATH SCATTER, retail HOLDING
/// a frozen counter where the port holds 0: mc2l23 slot 6 **242 rows**
/// t=17830..18071 (retail 299 flat, the record freed at t=18072);
/// mc2l19-taketwo slot 16 **264 rows** t=3257..3520 (retail 286);
/// mc2l22-new slot 106 **274 rows** t=20798..21071 (retail 257).
///
/// `MGC_NO_MC2_SCATTER_KEEPS_WINDOW=1` restores the invented clear.
pub(crate) fn no_mc2_scatter_keeps_window() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SCATTER_KEEPS_WINDOW").is_some())
}

/// ⭐⭐⭐ **THE WIZARD'S PURSE IS THE ENTITY WORD; `Mc2Rival::mana` IS
/// ONLY A MIRROR.** Retail has exactly ONE storage cell for a wizard's
/// mana — `type_entity_0x6E8E::mana_0x90_144` (@0x90, our `Ent::f140`)
/// — and every statement that moves it is a read-modify-write ON THAT
/// CELL: the housekeeping regen is `a1x->mana_0x90_144 +=
/// a1x->manaRegen_0x88_136` (EF:5426, `sub_12A70`) and the rebound
/// deflect's victim debit is `a2x->mana_0x90_144 -= a1x->
/// mana_0x90_144 >> 2` (EF:55274, `sub_68740`). The port keeps the
/// purse in the brain record `Mc2Rival::mana` and *publishes* it with
/// an ASSIGNMENT at the tail of the regen (`mc2/rivals.rs`,
/// `self.g.ent[i].f140 = r.mana`), so any entity-side write made by
/// ANOTHER slot's handler is silently reverted on the wizard's next
/// tick.
///
/// This is the same shape the target word already carries at the top
/// of [`World::mc2_rival_alive`] — "THE ENTITY WORD IS THE TARGET; the
/// brain record only MIRRORS it" — one field short.
///
/// WITNESS (mc2l22 free run, `MGC_WRITE_TRACE=611:f140`): rival 611
/// (3,1) is deflected by the (9,3) at slot 969 on t=3629 and retail's
/// `explain` reads `focus slot 611 … mana 534 -> 34` with the bolt
/// re-owned (`f1a 424 -> 611`) and re-homed (`target96 611 -> 424`) —
/// the port's own deflect debit matches it exactly, writing
/// `ent[611].f140 = 34` from slot 969's dispatch. Retail's t=3630 then
/// leaves the purse alone (`manaRegen` is 0 there), while the port's
/// t=3630 regen republishes the untouched shadow: `WRITE t=3630 slot
/// 611 f140 34 -> 534 by slot 611 (3,1) f70=1`. That is the take's
/// wall, `slot 611 mana: retail 34 port 534`, exactly +500 = the
/// quarter of the bolt's own 2000.
///
/// `MGC_NO_MC2_WIZ_PURSE_IS_ENTITY=1` restores the pre-dig behaviour
/// (the brain record is the master and the entity a write-only
/// mirror).
pub(crate) fn no_mc2_wiz_purse_is_entity() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_WIZ_PURSE_IS_ENTITY").is_some())
}

/// The plain-toast ink: retail draws it in the CLRD-0 code `0xF00` =
/// RGB(255,0,0) resolved to the nearest palette index (remc2
/// EF:22128). We carry the intended truecolor red.
const NOTIFY_RED: [u8; 3] = [255, 0, 0];

/// ⭐⭐⭐ **WHICH SPELLS PIN THE REGEN MID-BURST IS AN ENUMERATED
/// LIST.** `sub_68DE0` is not part of a shared skeleton — each of the
/// 26 manifestation handlers places its own call. Twenty put it AFTER
/// the `word_0x2E_46 == word_0x30_48` block, so every afforded tick
/// clamps a positive `manaRegen` to 0. These do NOT:
///
/// - `sub_6BCF0` (17), `sub_6BF30` (18), `sub_6C620` (21),
///   `sub_6C870` (22), `sub_6CAC0` (23) and `sub_6CFA0` (25) close the
///   block one brace LATER, with the call INSIDE it (EF:57510 opens
///   and EF:57531 calls, for 17) — they debit on the arm frame and
///   then leave the purse alone for the rest of their window;
/// - the CASTLE (2), whose `word_0x2E_46` is an upgrade LOCK and not
///   a countdown, so `sub_69AB0` only reaches `sub_68DE0` on the
///   fresh-cast sentinel.
///
/// HEAL (5) is off this list entirely: `sub_6A300` (EF:56432-77)
/// never calls `sub_68DE0` at all and stamps its own inline
/// accumulating debit.
///
/// Read by BOTH the live token pass and the conformance import's
/// `mc2_applied_mana_delta` — ⭐⭐⭐ a law landed on one call path is
/// not landed.
pub(crate) const NO_MID_BURST_REGEN_PIN: [usize; 7] = [2, 17, 18, 21, 22, 23, 25];

/// MC2's spellbook size. **NOT `mc1::spells::SPELL_COUNT`** (24):
/// MC2 adds spells 24 (Alliance) and 25, and every MC2 hand/book
/// bound in this module is the literal `26` this names. Introduced
/// after the pair importer's hand closure was found clamping the
/// human's hands with MC1's constant and dropping exactly those two.
pub(crate) const MC2_SPELL_COUNT: usize = 26;

/// The 26-spell `str_611` subset for one wizard (spell-XP trace §0).
/// All arrays are keyed by spell index 0..25 (`spell_t`).
#[derive(Clone, Copy)]
pub(crate) struct Mc2Spellbook {
    /// `SpellsEnabled_0x333`: pool slot of the spell's class-15
    /// manifestation, 0 = not learned.
    pub(crate) ent: [u16; 26],
    /// `spellsExperience_0x2CB` — volatile (this-level) XP.
    pub(crate) xp_vol: [i32; 26],
    /// `SpellExperience_0x263` — banked (campaign-carried) XP.
    pub(crate) xp_bank: [i32; 26],
    /// `SpellLevels_0x41D` — derived level 0..2.
    pub(crate) levels: [u8; 26],
    /// `array_0x437` — the selected tier per spell (≤ level).
    pub(crate) sel: [u8; 26],
    /// `SpellIndexLeft/Right_0x451/0x453` — quick-slot bindings
    /// (spell index, -1 = none).
    pub(crate) left: i8,
    pub(crate) right: i8,
    /// `array_0x3B5` — per-spell cycle-ring membership: 0 = none,
    /// 1 = the LEFT-button ring, 2 = RIGHT. Written ONLY by the
    /// pane's SHIFT+click (cmd 0x26, a raw byte store EF:37951);
    /// the normal equip routes never touch it. Deliberately kept on
    /// spell LOSS (sub_69300 clears possession + the equip pointer,
    /// not this) — the cycle walk skips unpossessed members.
    pub(crate) ring: [u8; 26],
}

impl Default for Mc2Spellbook {
    fn default() -> Self {
        Mc2Spellbook {
            ent: [0; 26],
            xp_vol: [0; 26],
            xp_bank: [0; 26],
            levels: [0; 26],
            sel: [0; 26],
            left: -1,
            right: -1,
            ring: [0; 26],
        }
    }
}

impl Mc2Spellbook {
    /// Hash-transparency gate: a never-touched book hashes like the
    /// pre-field struct (the MC1 goldens hold across the layout
    /// change — the bldgprm/spells pattern).
    pub(crate) fn is_pristine(&self) -> bool {
        self.ent == [0; 26]
            && self.xp_vol == [0; 26]
            && self.xp_bank == [0; 26]
            && self.levels == [0; 26]
            && self.sel == [0; 26]
            && self.left == -1
            && self.right == -1
            && self.ring == [0; 26]
    }
}

/// Manual, NOT derived: `ring` is folded only when populated, behind
/// a field tag (the transparent-while-clear discipline) — every book
/// pinned before the ring existed feeds the identical byte stream.
/// Field order up to `right` must stay the old derive order.
impl std::hash::Hash for Mc2Spellbook {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        let Mc2Spellbook {
            ent,
            xp_vol,
            xp_bank,
            levels,
            sel,
            left,
            right,
            ring,
        } = self;
        ent.hash(h);
        xp_vol.hash(h);
        xp_bank.hash(h);
        levels.hash(h);
        sel.hash(h);
        left.hash(h);
        right.hash(h);
        if *ring != [0; 26] {
            h.write_u8(0xB5);
            ring.hash(h);
        }
    }
}

/// Read-only spell-book snapshot for the app (pane tiles, XP bars,
/// hand indicators).
#[derive(Clone, Copy)]
pub struct Mc2BookView {
    pub owned: [bool; 26],
    pub levels: [u8; 26],
    pub sel: [u8; 26],
    /// Effective XP (banked + volatile) per spell.
    pub xp: [i32; 26],
    /// The per-tier XP thresholds (`xpos1_E`, the single-player
    /// ladder) — the flyout's unlock-progress bar bounds
    /// (EF:22633-71).
    pub xpos: [[i32; 3]; 26],
    /// The SELECTED tier's cast cost (`GetSpellManaCost_6D710` —
    /// castle rides the upgrade ladder).
    pub cost: [u32; 26],
    /// EVERY tier's cast cost — the flyout's broke test recomputes
    /// `GetSpellManaCost` per tier (EF:22609), not per selection.
    pub cost_tier: [[u32; 3]; 26],
    /// Cast-in-progress (`word_0x2E_46` > 0) — the HUD hand-panel
    /// highlight (retail's burst-counter frame swap).
    pub armed: [bool; 26],
    /// Retail's HUD EXPIRY-BLINK eligibility (DrawSpellIcon_2E260
    /// GameUI.cpp:351-54): a flag-4 long-runner whose cast window
    /// (`word_0x2E_46`) is live inside its last 31 ticks. While set,
    /// retail SKIPS the whole panel (and the CTRL-pane cell,
    /// EF:22493-99) on odd turns (`colorIndex_121[1]` = Turn & 1).
    pub expiring: [bool; 26],
    /// Retail's `canSummon`/`canSubSummon` PER TIER (the pane
    /// grey-out, EF:22503-08 grid / EF:22602-08 flyout): the tier's
    /// `maxManaLimit_A` castle-pool prerequisite is zero, or the own
    /// castle's stored mana covers it. False = the dark box +
    /// ghosted icon (SPELL_ICON_PANEL2 + transparent draw). Hand
    /// mana is NOT part of THIS flag, and on the GRID and the HUD
    /// hand panels retail truly ignores it (a broke-but-eligible
    /// spell stays lit with an empty shot meter) — but the FLYOUT
    /// tile ADDITIONALLY keys on `mana / cost` per tier
    /// (EF:22609/:22618, player retail-verified 2026-08-21): the
    /// app combines this flag with `cost_tier` there. The grid keys
    /// on the SELECTED tier (`castable[s][sel[s]]`).
    pub castable: [[bool; 3]; 26],
    pub left: i8,
    pub right: i8,
    /// `array_0x3B5` cycle-ring membership (0/1=left/2=right).
    pub ring: [u8; 26],
    /// The level is a cave (`isCaveLevel_D41B6`). Off-cave retail's
    /// grid draws Cave-In (25) as the plain empty box whatever the
    /// book says (EF:22429) and the pane refuses to select it
    /// (PlayerInput.cpp:849); the cast gate refuses it too
    /// (`mc2_cast_gate`).
    pub cave: bool,
}

/// One `sub_6DCA0` projectile arm (cast-path trace §2): the class-9
/// subtype to spawn, the impact (class, model), whether the tier's
/// `life_0x1A` charge byte rides along. The tier's `subSpellIndex_2`
/// payload ALWAYS rides — every effect-state skeleton copies it onto
/// the projectile (fireball EF:55864).
pub(crate) struct DispatchArm {
    pub(crate) subtype: u8,
    pub(crate) impact: (u8, u8),
    pub(crate) charge: bool,
}

/// Class-9 creator parameters (low-band trace + flyers trace Part 1):
/// (subtype, action, speed, maxLife, behavior row (always a real
/// str_D7BD6 index — a 255 would panic the BEHAVIOR lookup),
/// sprite). model = subtype throughout except 0x1C (model 28 rides
/// the fireball body). All creators: mana 50, no RNG.
const CREATORS: [(u8, u8, i16, u32, u8, u16); 20] = [
    (0, 0, 384, 21, 64, 340), // fireball (SummonFireball_4D2E0 EF:34729)
    // The BASIC possession bolt (`SummonManaPosession_4D3B0` EF:34764)
    // — tier `life_0x1A` 0 only, launched by `sub_69900` (EF:56039).
    // Same speed/life/row/sprite as its leveled (9,17) twin; the two
    // differ ONLY in action (1 vs 18) and in the ShiftRot fov factor
    // (5/2 vs 2 — see `mc2_spawn_cast_proj`).
    (1, 1, 384, 10, 61, 209),
    (2, 2, 384, 21, 60, 211), // earthquake shot (sub_4D470 EF:34788)
    (3, 3, 384, 21, 60, 76),  // meteor shot (sub_4D500 EF:34810)
    (4, 4, 384, 21, 60, 210), // volcano shot (sub_4D590 EF:34832)
    (5, 5, 384, 21, 60, 211), // crater shot (sub_4D620 EF:34854)
    // The DUEL DART (`sub_4D740` EF:34898) — spawned only by
    // `sub_6B610` (EF:57291), never by the `sub_6DCA0` band, which is
    // why it was missing from this table until mc2l6-rsg t=1326
    // recorded one at slot 855 (max_life 21 = 0x2000/384, sprite 213,
    // row 60, action 7).
    (7, 7, 384, 21, 60, 213),   // duel dart (sub_4D740 EF:34898)
    (8, 8, 384, 21, 63, 214),   // (sub_4D7D0 EF:34920)
    (9, 9, 384, 9, 63, 216),    // thunder bolt (sub_4D860 EF:34942)
    (12, 12, 384, 5, 60, 216),  // charged thunder (sub_4DA20 EF:35009)
    (17, 18, 384, 10, 61, 209), // possession (subtype 0x11, EF:35132)
    (22, 23, 384, 21, 60, 211), // gravity well (0x16, EF:35155)
    (23, 24, 384, 21, 60, 211), // tremor (0x17, EF:35199)
    (24, 25, 384, 20, 60, 281), // summon (0x18, EF:35221; maxLife &= 0xFC)
    (25, 26, 384, 10, 61, 321), // alliance (0x19, EF:35244)
    (26, 27, 384, 21, 60, 320), // whirlwind (0x1A, EF:35266)
    (28, 29, 384, 21, 64, 340), // charged fireball (0x1C, EF:34752)
    (29, 30, 384, 10, 60, 66),  // magic mine (0x1D, EF:35310)
    (30, 31, 384, 21, 60, 211), // cave-in (0x1E, EF:35288)
    (10, 10, 384, 21, 60, 18),  // castle ball (sub_4D900 EF:34965)
];

/// `MGC_NO_MC2_SPEED_TOKEN_LIVE_ACTSPEED=1` — the A/B arm for THE
/// SPEED TOKEN'S REWRITE OF THE CASTER'S `actSpeed_0x82_130` IS A LIVE
/// REGISTER EVERY LATER WALK SLOT READS (round 140, dig FALLPAIR):
/// restore the pre-dig behaviour where every human cast thunk took the
/// carpet's boost from the TICK-TOP pose snapshot, so a cast fired
/// from a token seated BETWEEN the speed token and the carpet read the
/// PREVIOUS tick's speed.
///
/// `GetScroll_69DB0` (EF:56595-56601 the per-tick sustain write,
/// EF:56616-19 the burst-END restore `speed_0xc_12 = minSpeed * sign`
/// then `actSpeed = speed_0xc_12`) writes `v1x->actSpeed_0x82_130` at
/// the SPEED TOKEN's own pool slot. BYTE-VERIFIED, shipped
/// `NETHERW.EXE` (file = VA + 0x24800): the function is file 0x8E5B0;
/// the sustain store is `66 89 86 82 00 00 00  mov [esi+0x82],ax` at
/// 0x8E737, and the expiry tail at 0x8E796-0x8E7B4 is
/// `jnz` (counter != 0) / `66 0f af be 84...  imul di,[esi+0x84]`
/// (sign × the caster's `minSpeed_0x84_132`) /
/// `66 89 78 0c  mov [eax+0xc],di` (`speed_0xc_12`) /
/// `66 89 86 82 00 00 00  mov [esi+0x82],ax` — the CASTER entity's
/// @0x82, written from the TOKEN's dispatch.
///
/// Every retail thunk that adds the caster's speed reads that same
/// register LIVE, and the port funnels all of them through three
/// sites ([`World::mc2_launch`], [`World::mc2_possess_launch`],
/// [`World::mc2_cast_fools_mana`]): `sub_6DCA0`'s tail
/// `v24 = a5 + v7x->actSpeed_0x82_130` (EF:44578-82; EXE 0x927AE-0x927E5
/// `mov eax,[ebp+0x24]` / `add` / floor 0x180 / ceiling 0x2000) with
/// `a5 = v1x->actSpeed_0x82_130` at all eleven `sub_6DCA0` call sites
/// (spell 9 is EF:57155; EXE 0x8F388 `mov ax,[ebx+0x82]; push eax`
/// inside `sub_6AB00` @0x8F300), the five bare
/// `spawned->actSpeed_0x82_130 += caster->actSpeed_0x82_130` arms —
/// `sub_69640` EF:56304, `sub_69900` EF:56399, `sub_6C170` EF:58023,
/// `sub_6CAC0` EF:58345, `sub_6CD20` EF:58426 — and Fool's Mana
/// (`sub_6C870` EF:58258, `v3 = 4 * v2x->actSpeed_0x82_130`).
/// ⚠ EF line numbers here are against the CURRENT
/// `reference/remc2/remc2/engine/EventsFunctions.cpp`; the older
/// `EF:56048`/`EF:44224` style cites elsewhere in this file are
/// against a different revision and do not line up.
///
/// The port already mails the token's write (`pending_speed_base`),
/// but only the CARPET's own dispatch consumed it, so a between-slot
/// cast never saw it. The mail IS retail's register value between the
/// token's write and the carpet's next dispatch, so reading it here is
/// the whole law.
pub(crate) fn no_mc2_speed_token_live_actspeed() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SPEED_TOKEN_LIVE_ACTSPEED").is_some())
}

/// A/B toggle for the CAMPAIGN-REPLAY XP GATE (`sub_6D8B0`'s own first
/// line): set `MGC_NO_MC2_REPLAY_XP_GATE` to restore the pre-dig
/// behaviour, where a level the save has ALREADY COMPLETED still
/// accrued spell XP.
///
/// `sub_6D8B0` (EF:58596) opens with
/// `if (!(x_D41A0_BYTEARRAY_4_struct.setting_38545 & 4)) { … }` — the
/// whole body, XP write included, is inside that test. Shipped
/// `NETHERW.EXE` file 0x920B0 (VA 0x6D8B0):
/// `mov 0x41a4,%eax` / **`testb $0x4,0x9691(%eax)` / `jne 0x6d9b3`**
/// (the epilogue) — BEFORE the `a1` null test at 0x6D8CE and before
/// the `class==3 && model==0` guard at 0x6D8E3/0x6D8ED. So on a
/// replayed level NOTHING happens: no `xp_vol` write, no spell-2
/// `SetSpell_6D5E0` re-sync, no `sub_6D9C0` relevel/notify.
///
/// The port already carries the flag as [`World::mc2_level_replayed`]
/// (`setting_38545 & 4`, derived from the capture by
/// `mgc_formats::mgcr::mc2_take_replayed`) and already honours its
/// other two consumers — the (14,5) scroll ctor pre-hide and the
/// scroll tick's instant hide + soft-kill. The XP half was the one
/// documented-but-unmodelled consumer.
pub(crate) fn no_mc2_replay_xp_gate() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_REPLAY_XP_GATE").is_some())
}

/// `MGC_NO_MC2_XP_AWARD_WHILE_DEAD=1` restores the INVENTED LIFE GUARD
/// [`World::mc2_award_xp`] used to open with
/// (`if self.players[hx].state != LifeState::Alive { return; }`).
///
/// ⭐⭐⭐ `sub_6D8B0` HAS NO LIFE TEST. The whole function is 264
/// bytes and every branch in it is enumerable; shipped `NETHERW.EXE`
/// file **0x920B0..0x921B7** (VA 0x6D8B0, MC2 file = VA + 0x24800),
/// prologue through the award:
/// ```text
///   920bc: a1 a4 41 00 00        mov    0x41a4,%eax
///   920c1: f6 80 91 96 00 00 04  testb  $0x4,0x9691(%eax)   ; replayed level
///   920c8: 0f 85 e5 00 00 00     jne    0x921b3             ;   → epilogue
///   920ce: 66 85 ff              test   %di,%di             ; owner != 0
///   920d1: 0f 84 dc 00 00 00     je     0x921b3
///   920dc: 8b 04 85 e4 a3 01 00  mov    0x1a3e4(,%eax,4),%eax  ; Entities[owner]
///   920e3: 80 78 3f 03           cmpb   $0x3,0x3f(%eax)     ; class == 3
///   920e7: 0f 85 c6 00 00 00     jne    0x921b3
///   920ed: 80 78 40 00           cmpb   $0x0,0x40(%eax)     ; model == 0
///   920f1: 0f 85 bc 00 00 00     jne    0x921b3
///   920f7: 8b 98 a4 00 00 00     mov    0xa4(%eax),%ebx     ; the player block
///   92106: 8b 8c 83 cb 02 00 00  mov    0x2cb(%ebx,%eax,4),%ecx  ; xp_vol[spell]
///   92113: 01 d1                 add    %edx,%ecx           ; += amount
///   92115: 89 4c 83 68           mov    %ecx,0x68(%ebx,%eax,4)   ; (0x263+0x68)
/// ```
/// Four gates: the replayed-level bit, `owner != 0`, `class3f == 3`,
/// `model40 == 0`. The owner record's `life_0x8` is never loaded, and
/// neither is any player-block state byte — `0x3f`/`0x40`/`0xa4` are
/// the only reads off `%eax` in the whole prologue. A dead human is
/// still class 3 model 0 in the pool (retail never re-classes the
/// carpet), so retail credits XP THROUGH the death.
///
/// WITNESS — mc2l24-crazy t=3209, the tick the human dies (`explain`:
/// `notify "has died."`, every `spell_ent` reset, the carpet's
/// `flags.b0_x20 0 -> 1`) and retail's `xp_vol[1] 17 -> 18` lands on
/// the SAME tick. Pair mode (`verify-deltas`, `MGC_RAW_SHADOW=1`)
/// reports exactly one row on the slice 3100..3400:
/// `wiz 0 xp_vol t=3208 [1]: retail 18 port 17`, and the free run
/// carries that single missed +1 for the remaining 50,187 boundaries
/// of the take (the r148 free census's
/// `xp_vol 180,187 rows t=3209..79064`). With the guard dropped the
/// pair row and the whole free-run `xp_vol[1]` family go to zero.
pub(crate) fn no_mc2_xp_award_while_dead() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_XP_AWARD_WHILE_DEAD").is_some())
}

/// ⭐⭐⭐ **SHIELD IS PAID ON THE *ABSORB*, NOT ON THE CAST — AND
/// `sub_6A480` NEVER AWARDS AT ALL.** The XP call site for spell 6 is
/// not in the shield's manifestation handler; it is in the WIZARD'S
/// OWN DAMAGE-SETTLE ROUTINE `sub_5EFA0` (EF:61076-78; banner
/// 0005EFA0 at EF:61012), one statement INSIDE the two-stage absorb
/// gate. The shield only earns its owner XP when it actually eats a
/// letter.
///
/// An `e8 rel32` scan of the shipped image for `sub_6D8B0`
/// (VA 0x6D8B0, file 0x920B0) finds **33** callers and exactly ONE of
/// them pushes the spell immediate 6 — file **0x8399D**, inside
/// `sub_5EFA0` (file 0x837A0 = VA 0x5EFA0). Shipped `NETHERW.EXE`,
/// the whole gate:
/// ```text
///   83962: 66 8b 43 62     mov    0x62(%ebx),%ax      ; word_0x62_98 (attacker)
///   83966: 66 85 c0        test   %ax,%ax
///   83969: 0f 84 d4 01 ..  je     0x83b43             ; no letter -> skip
///   8396f: 8a 6b 0d        mov    0xd(%ebx),%ch       ; byte[1]
///   83972: 66 89 43 26     mov    %ax,0x26(%ebx)
///   83976: f6 c5 40        test   $0x40,%ch           ; CHARGED?
///   83979: 75 06           jne    0x83981             ;   -> award
///   8397b: f6 43 0e 40     testb  $0x40,0xe(%ebx)     ; ARMED?
///   8397f: 74 72           je     0x839f3             ;   neither -> no award
///   83981: a1 a0 41 00 00  mov    0x41a0,%eax
///   83986: 89 da           mov    %ebx,%edx
///   83988: 05 8e 6e 00 00  add    $0x6e8e,%eax
///   8398d: 29 c2           sub    %eax,%edx           ; a1x - struct_0x6E8E
///   8398f: b9 a8 00 00 00  mov    $0xa8,%ecx
///   83994: 89 d0           mov    %edx,%eax
///   83996: c1 fa 1f        sar    $0x1f,%edx
///   83999: f7 f9           idiv   %ecx                ; -> the victim's slot
///   8399b: 6a 01           push   $0x1                ; amount
///   8399d: 6a 06           push   $0x6                ; SPELL 6
///   8399f: 50              push   %eax                ; owner = victim slot
///   839a0: e8 0b e7 00 00  call   0x920b0             ; sub_6D8B0
///   839a5: 8a 63 0d        mov    0xd(%ebx),%ah       ; …then the CHARGED/ARMED split
/// ```
/// The award sits ABOVE the split, so BOTH stages pay: a Shield III
/// window that nulls one letter (ARMED) and then quarters the next
/// (the promoted CHARGED stage) credits **+1 twice**, a few ticks
/// apart, tens of ticks after the cast — and a shield window that is
/// never hit credits NOTHING. `sub_6A480`, the shield's own
/// manifestation body, contains no `sub_6D8B0` call in its 0x8E...
/// range at all (it is not among the 33 call sites).
///
/// The port awarded once, at CAST time, out of `mc2_spell_fire`'s
/// `6 =>` arm — the classic sibling defect the tree already records
/// for Heal ("NEVER REACHES HERE … the XP is gated on actually
/// healing", [`World::mc2_heal_token_tick`]).
///
/// WITNESS — mc2l16 pair mode (`verify-deltas`, `MGC_RAW_SHADOW=1`),
/// `wiz 0 xp_vol[6]`, six episodes, every one the same three-row
/// signature (tick: retail / port):
/// `13025 0/1 · 13060 1/0 · 13064 2/1` — the port credits at the
/// cast and retail credits on the two absorbs 35 and 39 ticks later.
/// Also 13313/13335/13339, 14141/14191/14194, 20507/20540/20541,
/// 23601/23645/23650 and the singles 23068 / 24057.
///
/// Set `MGC_NO_MC2_SHIELD_XP_ON_ABSORB` to restore the cast-time
/// award and drop the absorb one.
pub(crate) fn no_mc2_shield_xp_on_absorb() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_SHIELD_XP_ON_ABSORB").is_some())
}

/// ⭐⭐ **REBOUND IS THE SAME SHAPE AS SHIELD — AND THE PORT WAS
/// PAYING IT TWICE.** `sub_6AA00`, the rebound manifestation body
/// (banner EF:57071; VA 0x6AA00 = file 0x8F200, up to `sub_6AD60` at
/// file 0x8F560 — both verified function starts, `53 56 57 55 89 e5`
/// after `8d 40 00` padding), contains **no `sub_6D8B0` call**: the
/// `e8 rel32` scan of the shipped image lists 33 callers of the award
/// routine and none of them lies in that range (the neighbours are
/// file 0x8D00C and 0x8E6E0). Spell 8's ONLY award site is the
/// DEFLECT itself, inside the shared mover gate `sub_68740` (banner
/// EF:55571, the award at EF:55624; file 0x8CF40 = VA 0x68740):
/// ```text
///   8cfe7: 8b 15 a0 41 00 00  mov    0x41a0,%edx
///   8cfed: 8b 45 18           mov    0x18(%ebp),%eax     ; the VICTIM record
///   8cff0: 81 c2 8e 6e 00 00  add    $0x6e8e,%edx
///   8cff6: 29 d0              sub    %edx,%eax
///   8cffa: b9 a8 00 00 00     mov    $0xa8,%ecx          ; stride 168
///   8cfff: c1 fa 1f           sar    $0x1f,%edx
///   8d002: f7 f9              idiv   %ecx                ; -> the deflector's slot
///   8d007: 6a 01              push   $0x1                ; amount
///   8d009: 6a 08              push   $0x8                ; SPELL 8
///   8d00b: 50                 push   %eax
///   8d00c: e8 9f 50 00 00     call   0x920b0             ; sub_6D8B0
///   8d011: 8b 83 90 00 00 00  mov    0x90(%ebx),%eax     ; …then mana -= mana/4
///   8d02d: 66 8b 43 1c        mov    0x1c(%ebx),%ax      ; …and the yaw flip
/// ```
/// [`Gen::mc2_rebound_deflect`] already mails exactly that award
/// (`mc2_cast_xp.push((PLAYER_TARGET, 8, 1))` on the human arm), so
/// `mc2_spell_fire`'s `8 =>` cast-time credit was a SECOND, invented
/// one: an armed rebound window that deflects nothing earned XP, and
/// one that deflects earned +2.
///
/// Set `MGC_NO_MC2_REBOUND_XP_ON_DEFLECT` to restore the extra
/// cast-time award.
pub(crate) fn no_mc2_rebound_xp_on_deflect() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("MGC_NO_MC2_REBOUND_XP_ON_DEFLECT").is_some())
}

impl Gen {
    /// The shared class-9 creator body (low-band trace preamble):
    /// NewEvent + fields + `byte[0] &= 0xF7` + map link + life copy +
    /// sprite. Launch yaw/pitch are the launcher's job.
    pub(crate) fn mc2_spawn_cast_proj(
        &mut self,
        subtype: u8,
        x: u16,
        y: u16,
        z: i16,
    ) -> Option<usize> {
        let (_, action, speed, life, row, sprite) = *CREATORS.iter().find(|c| c.0 == subtype)?;
        let i = self.new_event()?;
        {
            let e = &mut self.ent[i];
            e.class64 = 9;
            // Subtype 0x1C is the fireball body under model 28
            // (sub_4D380's override) — model = subtype otherwise.
            e.model65 = subtype;
            e.tick70 = action;
            e.f126 = speed;
            e.f128 = speed;
            e.f140 = 50;
            e.max_life = life;
            e.row156 = row;
            e.flags = (e.flags & !8) | super::proj::F_MC2PROJ;
            // The BASIC possession bolt is the one class-9 creator
            // that narrows `xtype_0x41_65` off the NewEvent −1
            // wildcard: `SummonManaPosession_4D3B0` stamps 10
            // (EF:34775; the leveled (9,17) `sub_4DDD0` does NOT).
            // Retail's own possession probe `sub_108B0` never reads
            // it — the lane only bites if the bolt ever runs the
            // generic `sub_10780` — so `mc2_flyer_tick` skips
            // `mc2_proj_filter` on the claim arm to keep it inert
            // exactly like retail (worm/building claims survive).
            if subtype == 1 {
                e.f66 = 10;
            }
        }
        self.link(i, x, y, z);
        self.refill_life(i);
        self.mc2_set_sprite(i, sprite);
        // The POSSESSION pair alone re-boxes after the sprite set
        // (`SetEntityShiftRot_49EA0`): the basic (9,1) takes
        // `(2*pitch, 5*fov/2)` (EF:34781) and the leveled (9,17)
        // `(2*pitch, 2*fov)` (EF:35148). Sprite 209's row is
        // (speed_6 0, rotSpeed_8 150) → pitch/roll 0 either way, fov
        // 187 vs 150 — the z half-extent the claim probe and the cave
        // ceiling glide both read. Every other class-9 creator stops
        // at `SetEntityIndexAndRot_49CD0`.
        if matches!(subtype, 1 | 17) {
            let e = &self.ent[i];
            let shift = e.f80.wrapping_mul(2);
            let fov = if subtype == 1 {
                5 * e.f84 / 2
            } else {
                2 * e.f84
            };
            self.mc2_shift_rot(i, shift, fov);
        }
        Some(i)
    }

    /// Fool's Mana retaliation (`sub_36680` EF:26615), run from a
    /// (10,57) sphere's own tick while the ch1 claim latch
    /// (`word_0x68_104` → the ch1 mail SOURCE, @0x68) is set —
    /// mc1/combat.rs `ball_tick`. Retail field homes, all imported:
    /// parentId = id24 (@0x28), tier = f71 (@0x46), payload = f44
    /// (@0x2A), counter = f26 (@0x10). Returns true when the sphere is
    /// spent and must be consumed. The ONLY no-trap arm is
    /// `parentId == claimer` (EF:26623) — it clears the channel and the
    /// sphere lives on; there is no "is this a cast decoy" gate, which
    /// is why the AUTHORED ground spheres (parentId 0, tier 0) trap
    /// every possessor. Per tier: 0 → one fireball at the possessor,
    /// done; 1 → a fireball every other tick, up to 8, then done; 2/3 →
    /// one lightning bolt, then despawn after two ticks; >3 → nothing,
    /// ever (retail's fallthrough returns 0 and never clears the latch,
    /// so the sphere freezes claimed). The projectile homes the
    /// possessor (docs/spell-audit/fools-mana.md §2b).
    pub(crate) fn mc2_fools_retaliate(&mut self, i: usize, ctx: &MobCtx) -> bool {
        let claimer = self.ent[i].mail[1].1;
        if self.ent[i].id24 == claimer {
            // EF:26623-27: the owner cannot be fooled by its own —
            // clear the channel (amount and source) and carry on.
            self.ent[i].mail[1] = (0, 0);
            return false;
        }
        let tier = self.ent[i].f71;
        match tier {
            0 => {
                self.mc2_fools_bolt(i, 0, (10, 0), claimer, ctx);
                // XP lands when the trap SPRINGS (EF:26636), not on
                // the cast.
                self.mc2_fools_award(i);
                true
            }
            1 => {
                let c = self.ent[i].f26;
                self.ent[i].f26 = c.wrapping_add(1);
                if c >= 8 {
                    self.mc2_fools_award(i); // after the 8th (EF:26646)
                    return true;
                }
                // The fireball fires when the POST-increment counter
                // is even = old counter ODD (EF:26648 `!(++c & 1)`).
                if c & 1 != 0 {
                    self.mc2_fools_bolt(i, 0, (10, 0), claimer, ctx);
                }
                false
            }
            2 | 3 => {
                let c = self.ent[i].f26;
                self.ent[i].f26 = c.wrapping_add(1);
                if c == 0 {
                    self.mc2_fools_bolt(i, 9, (10, 23), claimer, ctx);
                    return false;
                }
                // Despawn at old counter 2 (`v3+1 > 2`, EF:26661).
                let done = c > 1;
                if done {
                    self.mc2_fools_award(i); // on despawn (EF:26663)
                }
                done
            }
            // Tier > 3 falls out of `sub_36680` with v5 = 0 (EF:26665):
            // no trap, no transfer, and the latch is never cleared —
            // the sphere is claimed forever and never moves again.
            _ => false,
        }
    }

    /// `sub_6D8B0(parentId, 0x16, 1)` at the trap's SPEND points —
    /// via the XP mail (this is `Gen`; the book lives on `World`).
    /// parentId rides id24 (@0x28); an AUTHORED sphere carries its own
    /// slot there (retail: 0), so the level's bait credits nobody —
    /// which is what `sub_6D8B0(0, …)` does.
    fn mc2_fools_award(&mut self, i: usize) {
        let owner = self.ent[i].id24;
        if self.is_human_tag(owner) {
            self.mc2_cast_xp.0.push((owner, 22, 1));
        }
    }

    /// Spawn one Fool's-Mana retaliation projectile from the sphere,
    /// homing the possessor: fireball (`sub_36770`, subtype 0, impact
    /// (10,0), sound 9) or thunder bolt (`sub_36850`, subtype 9, impact
    /// (10,23)). Owner = the trap's parentId (id24) so the flyer's
    /// autoaim never turns it on the caster; the tier's damage payload
    /// (`subSpellIndex_0x2A_42` → f44) rides onto the projectile's own
    /// f44 (EF:26691/26722). Retail's `sub_655C0` aims at the CLAIMER
    /// entity — the human wizard included (retail humans are in-pool);
    /// our out-of-pool human resolves through the ctx pose, the same
    /// sentinel resolution every creature attack aim uses
    /// ([`Gen::mc2_target`]). A reaped pool claimer falls back to the
    /// sphere's launch heading.
    fn mc2_fools_bolt(
        &mut self,
        i: usize,
        subtype: u8,
        impact: (u8, u8),
        claimer: u16,
        ctx: &MobCtx,
    ) {
        let (x, y, z, owner, payload, heading, raw_z) = {
            let e = &self.ent[i];
            // The MUZZLE LIFT (`position.z += array_0x52_82.fov`,
            // EF:26688 fireball / EF:26718 lightning): the bolt leaves
            // from the TOP of the launcher's own box, not its origin.
            // `a1x` there is the SPHERE, so the fov is the sphere's —
            // exactly the same law shape as the possession cast's
            // `position.z += a2x->array_0x52_82.fov` (EF:56054 /
            // EF:55969), where the launcher is the wizard. mc2l24
            // t=1322: retail's fireball leaves at z=898 off a z=846
            // sphere with afov 42.
            //
            // The self-detonation this was deferred over is NOT a
            // probe-filter gap: retail's `sub_10780` (EF:3739) has no
            // launcher exclusion at all. What keeps the bolt off its
            // own sphere is (a) the tier-0 sphere is UNMAPPED and
            // class-zeroed at the end of its OWN tick — the entity
            // walk runs `sub_57F20` (Events.cpp:551, :5209:
            // `SetMapEntity_57E50` + `class = 0` + free-stack push)
            // the instant `DisableEntityDrawing04_57F10` latches
            // byte[1]&4 — and (b) retail probes ONCE, at the END of a
            // full 384-unit step (`sub_65C20` EF:63126-29: MoveEntity,
            // CopyEntityPosition, THEN `sub_10780`). Our soft kill
            // leaves the sphere linked until the tick-top reap, and
            // our anti-tunnel chord march probes sub-steps the retail
            // probe never visits — so the launcher is excluded here,
            // at the source, by owner identity: the bolt inherits the
            // sphere's `id24`, and `victim_scan`'s `c.id24 != id`
            // (retail's `a1x->id_0x1A_26 != v5x->id_0x1A_26`,
            // EF:3769) then drops it, together with the co-located
            // (10,0) consume poof once that inherits the owner too.
            let lift = e.f84 as i16;
            (
                e.x,
                e.y,
                e.z.wrapping_add(lift),
                e.id24,
                e.f44 as i32,
                e.f30,
                e.z,
            )
        };
        let Some(pr) = self.mc2_spawn_cast_proj(subtype, x, y, z) else {
            return;
        };
        // The fireball's water-spawn splash (EF:26690-95, inside the
        // spawn-success arm): a (10,5) splash + sound 27 when the
        // sphere sits on water.
        //
        // ⭐ SPAWN + SOUND ONLY — an ABSENCE in an enumerated store
        // list, disassembled in full at [`no_fools_splash_arm`]: the
        // splash takes NO `id_0x1A_26`, so like the fireball above it
        // it is a stranger to everyone, its OWN slot included.
        if subtype == 0 && self.cap_bit(x, y) == 1 {
            if let Some(s) = self.mc2_spawn_splash(x, y, z) {
                if no_fools_splash_arm() {
                    self.ent[s].id24 = owner;
                }
                self.snd(27, s);
            }
        }
        // ⭐⭐⭐ THE AIM IS TAKEN AT THE **UNLIFTED** z, AND THE MUZZLE
        // LIFT IS APPLIED AFTER IT. Both retaliation spawners spell the
        // order out and both put the lift LAST:
        //   sub_36770  sub_655C0(v1x, v2x);                    EF:26683
        //              v1x->yaw   = v1x->roll;                 EF:26684
        //              v1x->pitch = v1x->fov;                  EF:26685
        //              v1x->position.z += a1x->array_0x52_82.fov; EF:26686
        //   sub_36850  sub_655C0(v1x, v3x);                    EF:26717
        //              … v2x->pitch = v2x->fov;                EF:26721
        //              v2x->position.z += a1x->array_0x52_82.fov; EF:26725
        // and `sub_655C0` reads `a1x->position_0x4C_76` (EF:62775-76),
        // which is still the SPHERE's own z at that point. Aiming from
        // the lifted origin tilts every retaliation bolt further down.
        //
        // ⭐ AND `sub_655C0` RAISES THE TARGET: its first statement is
        // `sub_65580(a2x)` (EF:62774), `position.z += array_0x52_82.yaw`
        // unless model 2 (EF:62755-56) — the port's out-of-pool human is
        // a raised victim exactly like the pool branch, and its `ayaw`
        // IS `PLAYER_HH` (mc2l24 t=1024 slot 116 records ayaw = 100).
        //
        // mc2l24 pair 1024→1025 slot 70 is both halves at once: sphere
        // 78 at (22273, 26950, **2856**) with afov 42, human 116 at
        // (23386, 30122, 2037) ayaw 100 → dh 3361, dz 2856 − 2137 =
        // 719 ⇒ retail's pitch **68**. Aiming from the lifted 2898 at
        // the unraised 2037 gives dz 861 ⇒ the port's 82.
        let (asrc, praise) = if no_fools_aim_phase() {
            (z, 0i16)
        } else {
            (raw_z, crate::mc1::combat::PLAYER_HH as i16)
        };
        let (yaw, pitch) = if let Some(hx) = self.hx_of_tag(claimer) {
            let (tx, ty, tz) = (
                self.seat_at(ctx, hx).0,
                self.seat_at(ctx, hx).1,
                self.seat_at(ctx, hx).2.wrapping_add(praise),
            );
            let yaw = Self::angle_between(x, y, tx, ty);
            let dh = Self::isqrt(Self::dist2_sq(x, y, tx, ty) as u32) as i32;
            (yaw, Self::pitch_toward(asrc, tz, dh))
        } else if (claimer as usize) < self.ent.len()
            && self.ent[claimer as usize].flags & 0x400 == 0
        {
            let t = &self.ent[claimer as usize];
            // `sub_65580` — the model-2 exemption is the castle's.
            let tz = if no_fools_aim_phase() {
                t.z.wrapping_add(t.f78 as i16)
            } else {
                t.aim_z()
            };
            let (tx, ty) = (t.x, t.y);
            let yaw = Self::angle_between(x, y, tx, ty);
            let dh = Self::isqrt(Self::dist2_sq(x, y, tx, ty) as u32) as i32;
            (yaw, Self::pitch_toward(asrc, tz, dh))
        } else {
            (heading, 0)
        };
        {
            let e = &mut self.ent[pr];
            // ⭐⭐⭐ A SPLIT IN A SIBLING PAIR: THE FIREBALL IS A
            // STRANGER TO EVERYONE. `sub_36850` (the LIGHTNING
            // retaliation) writes `v1x->id_0x1A_26 = a1x->id_0x1A_26`
            // (EF:26714) — the SPHERE's own `@0x1A`, not its
            // `parentId_0x28_40`. `sub_36770` (the FIREBALL,
            // EF:26671-96) contains NO `id_0x1A_26` statement at all,
            // so its flyer keeps `NewEvent_4A050`'s ctor seed
            // `id_0x1A_26 = <own pool index>` (Events.cpp:570) — which
            // `Gen::new_event` already lays down.
            //
            // The port stamped the sphere's `id24` on BOTH arms, and
            // on a (10,57) that fused field carries retail's
            // `parentId_0x28_40` = the wizard who cast the Fool's
            // Mana. Owner immunity then spared everything of that
            // wizard's, and the loudest reader is `sub_10C80`'s ch0
            // CASTLE pre-pass (EF:4062-72), whose only gate is
            // `iix->id_0x1A_26 != a1x->id_0x1A_26` — so a retaliation
            // fireball retail bills against the caster's OWN castle
            // (the castle's `sub_106C0` box is ~26 tiles wide) cost
            // the port nothing.
            // WITNESS mc2l6-rsg t=16600: bolt 187 (born 16597 off
            // sphere 530) — retail `@0x1A` 187, port 343; castle 63
            // (`id24` 343) takes `mail0 = (500, 187)` in retail and
            // none in the port, and its life forks 156100/156600 at
            // t=16601. That is the take's frontier wall.
            // ⭐ AND EVEN THE SIBLING'S COPY IS OF `@0x1A`, NOT
            // `parentId`. `sub_36850` copies the SPHERE's `@0x1A`
            // (0x5B093/0x5B097), which for a (10,57) is its own pool
            // slot — see [`no_fools_lightning_id`]. Writing `owner`
            // (the port's fused `id24` = the Fool's-Mana caster) gave
            // the thunder bolt the caster's friendly-fire immunity.
            if no_fools_bolt_stranger() {
                e.id24 = owner;
            } else if subtype != 0 {
                e.id24 = if no_fools_lightning_id() {
                    owner
                } else {
                    i as u16
                };
            }
            e.f68 = impact.0;
            e.f69 = impact.1;
            e.f44 = payload.clamp(0, u16::MAX as i32) as u16;
            e.f30 = yaw;
            e.f34 = yaw;
            e.f32 = pitch;
            e.f36 = pitch;
            e.f146 = claimer; // homing lock on the possessor
        }
        // The LIGHTNING bolt re-rows to the homing row 64 and stamps
        // the claimer's class/model as its xtype/xsubtype filter
        // (sub_36850 EF:26701-20); the fireball stamps none.
        if subtype == 9 {
            let (tc, tm) = if self.is_human_tag(claimer) || claimer as usize >= self.ent.len() {
                (3, 0)
            } else {
                (
                    self.ent[claimer as usize].class64,
                    self.ent[claimer as usize].model65,
                )
            };
            let e = &mut self.ent[pr];
            e.row156 = 64;
            e.f66 = tc;
            e.f67 = tm;
        }
        if subtype == 0 {
            // Sound 9 rides the NEW fireball (EF:26689), not the
            // sphere.
            self.snd(9, pr);
        }
    }
}

/// See [`World::mc2_v34_token_pre`].
#[derive(Clone, Copy)]
pub(crate) enum TokenV34 {
    Idle,
    MeteorFire(u32),
    Other,
}

impl World {
    // ---- SetSpell / mana cost / level law --------------------------------

    /// `GetSpellManaCost_6D710` (L:1714): the tier's `manaCost_6`;
    /// the castle spell (2) rescales to the upgrade ladder at the
    /// OWN castle's current entity level (L:1729-55 — the verbatim
    /// table, default rung 300M), plus the `byte_0x1BE_446` +3000
    /// RE-CAST SURCHARGE (see [`World::mc2_recast_surcharge`]).
    pub(crate) fn mc2_spell_mana_cost(&self, hx: Hx, spell: usize, tier: usize) -> i32 {
        self.mc2_spell_mana_cost_at(hx, spell, tier, self.seat_castle(hx))
    }

    /// [`Self::mc2_spell_mana_cost`] against an EXPLICIT castle —
    /// retail's `CastleEntityIndex_0x3A_58` as it stood when the
    /// pricing ran. The ladder-sync drain passes the castle whose
    /// stamp mailed it, because by drain time that castle may already
    /// carry `0x400` and vanish from `seat_castle()` (the DYING
    /// level-0 castle — see the drain's own note in world.rs).
    pub(crate) fn mc2_spell_mana_cost_at(
        &self,
        hx: Hx,
        spell: usize,
        tier: usize,
        castle: Option<usize>,
    ) -> i32 {
        let Some(row) = self.g.assets.spells.get(spell) else {
            return 0;
        };
        let base = row.tiers[tier.min(2)].mana_cost;
        if spell == 2 {
            // THE +3000 RE-CAST SURCHARGE (L:1723-26 / L:1776-79) —
            // UNCONDITIONAL, and deliberately NOT on the (since
            // retired, 2026-09-07) `castle_recast_cost` toggle
            // (player-ruled 2026-08-23c, reversing the initial ruling
            // once the provenance was understood). That toggle
            // relieved MC1's first-castle
            // LOCKOUT, which is an unpatched retail BUG: MC1 never
            // re-stamps the cached price on castle death, so ANY loss
            // prices the rebuild at CAP[0] = 5,000 against a 1,000
            // purse. THIS is not a bug — it is MC2's DESIGNED
            // replacement for that accident: the base drops to 1,000,
            // a destroyed castle is immediately rebuildable, and the
            // +3,000 lands only when the latch is set, which only a
            // VOLUNTARY level-1 demolish can do (`PlayerAction 0x2A`,
            // EF:37993-95 — no other writer exists). Suppressing
            // designed behaviour under a bug-relief switch would make
            // the patched arm LESS faithful for no gameplay reason.
            // `MGC_NO_MC2_RECAST_SURCHARGE=1` remains the A/B arm.
            let surcharge =
                self.mc2_recast_surcharge[hx] && !crate::engine::world::mc2_recast_surcharge_off();
            // `GetSpellManaCost_6D710` (L:1714-85): the castle upgrade
            // cost is the OWN castle level's ladder rung times the
            // spell-LEVEL (tier) multiplier — this is why a fire/
            // lightning castle (tier 1/2) needs far more possessed-
            // but-uncollected mana than the pool can hold, so you
            // can't over-build (docs/spell-audit/castle-and-cost.md).
            // With no own castle, retail returns the tier's base
            // manaCost — NOT a ladder rung, and with NO tier multiply
            // (L:1717-21); the surcharge still applies on top.
            let Some(c) = castle else {
                return base.saturating_add(if surcharge { 3000 } else { 0 });
            };
            // The MC2 PRICE ladder — its own per-game definition, see
            // [`crate::mc2::castle::MC2_CASTLE_COST`]. NOT MC1's
            // `Gen::CASTLE_CAP` (which fuses price and capacity) and
            // NOT MC2's own `MC2_CASTLE_CAP` (capacity).
            let lvl = self.g.ent[c].f26.clamp(0, 7) as usize;
            let mut result = crate::mc2::castle::MC2_CASTLE_COST[lvl];
            if lvl < 7 {
                // The tier multiplier (L:1725-33): ×1.25 / ×1.5 via the
                // 320/256 · 384/256 fixed-point idiom (round toward
                // zero; the ladder rungs divide evenly). Level 7 (the
                // 300M cap) takes no multiply.
                result = match tier.min(2) {
                    1 => (result * 320) >> 8,
                    2 => (result * 384) >> 8,
                    _ => result,
                };
            }
            // AFTER the multiply, and at LEVEL 0 ONLY. Retail's
            // `add3000` local is provably still false on the level-≥7
            // and level-≠0 returns (L:1758-72) — it is set only in the
            // castle-less arm (L:1723-26) and in the level-0 tail
            // (L:1776-79). Measured: galore t=32600, L0 at tier 2,
            // retail 4500 = (1000·384>>8) + 3000, which is what pins
            // the multiply as preceding the add.
            if lvl == 0 && surcharge {
                result += 3000;
            }
            return result.clamp(0, i32::MAX as i64) as i32;
        }
        base
    }

    /// The MC2 per-tier spell name shown on the CTRL-pane hover and the
    /// spell-change toast (`SetSpellHelpPopupCoordinates_88D40` case 0 /
    /// EF:37925): resolve the LIVE `hint_text` lang index (post the
    /// Day/non-Day `level_init_patch`) to its retail `L2.TXT` string, so
    /// each tier reads as its own name ("Possession"/"Mana Magnet"/"Mana
    /// Lock"), not one generic label. Empty when mc2 spell data is absent
    /// (docs/spell-audit/spell-names.md).
    pub fn mc2_spell_name(&self, spell: usize, tier: usize) -> &'static str {
        let idx = self
            .g
            .assets
            .spells
            .get(spell)
            .map_or(0, |r| r.tiers[tier.min(2)].hint_text);
        super::spells::lang(idx)
    }

    /// The level-up banner text (`sub_6DC40_improve_ability` EF:44011):
    /// "Your ability to cast %s has improved." with the spell's UPPERCASE
    /// base name (lang 160+spell) substituted.
    pub fn mc2_relevel_message(&self, spell: usize) -> String {
        super::spells::lang(159).replace("%s", super::spells::lang(160 + spell as i16))
    }

    /// `SetSpell_6D5E0` (L:1505): wire the tier's SPELLS row into the
    /// manifestation. Mid-cast, the change is deferred (`word_0x2C_44
    /// = tier+1`, applied by `sub_6D880` when the timer expires).
    pub(crate) fn mc2_set_spell(&mut self, hx: Hx, m: usize, tier: u8) {
        self.mc2_set_spell_at(hx, m, tier, self.mc2_price_castle(hx));
    }

    /// ⭐⭐⭐ THE CASTLE-SPELL PRICE READS THE **REGISTER**, NOT THE POOL.
    /// `GetSpellManaCost_6D710` (Level.cpp:1723-27) resolves the
    /// pricing castle as
    /// `Entities_EA3E4[event->dword_0xA4_164x->CastleEntityIndex_0x3A_58]`
    /// and takes the "no castle" arm on `entity2 <= Entities_EA3E4[0]`
    /// — shipped `NETHERW.EXE` 0x91f59-0x91f74
    /// (`mov 0xa4(%esi),%edx` / `mov 0x3a(%edx),%bx` /
    /// `mov 0x1a3e4(,%ebx,4),%ebx` / `cmp %edi,%ebx` / `jbe`). There is no
    /// class, model, owner or reap test anywhere in it: the word alone
    /// decides, and the LEVEL rung then comes off that record's
    /// `dword_0x10_16`.
    ///
    /// [`World::seat_castle`] is a POOL SCAN and returns the
    /// lowest-numbered live (3,2) the human owns. Round 136's law:
    /// A POOL SCAN RETURNS THE LOWEST-NUMBERED MATCH, A REGISTER THE
    /// CHOSEN ONE — ONLY A CASTLE SPLIT MAKES THEM DIFFER. mc2l12 has
    /// one: from ~t=19714 the orphan's UNCONDITIONAL teardown clear
    /// left the register 0 with castle 293 still standing, so every
    /// retail re-price in that window takes the NO-CASTLE arm (raw
    /// `manaCost_6` + the 3000 surcharge = 5000) while the port keeps
    /// pricing off the standing castle's rung.
    ///
    /// Witness, mc2l12 pair 43127→43128, slot 117 (15,2) owner 114:
    /// retail `mana_max` 5000 / `mana` 49, port 300000000 / 2970297
    /// (the level-7 rung). The same lane carries 36 of the take's
    /// heads, every one of them a price stamped at a `SetSpell` tick.
    ///
    /// `MGC_NO_MC2_SPELL_PRICE_REGISTER=1` restores the pool scan.
    pub(crate) fn mc2_price_castle(&self, hx: Hx) -> Option<usize> {
        if crate::engine::world::no_mc2_spell_price_register() {
            return self.seat_castle(hx);
        }
        self.player_castle_bound(hx)
    }

    /// [`Self::mc2_set_spell`] pricing against an EXPLICIT castle —
    /// the ladder-sync drain's entry point (see world.rs's drain).
    pub(crate) fn mc2_set_spell_at(&mut self, hx: Hx, m: usize, tier: u8, castle: Option<usize>) {
        let spell = self.g.ent[m].model65 as usize;
        let Some(row) = self.g.assets.spells.get(spell) else {
            return;
        };
        let count = (row.byte_0 as i16).max(1);
        let t = (tier as i16).min(count - 1).max(0) as usize;
        if self.g.ent[m].f26 > 0 {
            self.g.ent[m].f44 = (t + 1) as u16;
            return;
        }
        let sub = row.tiers[t];
        let cost = self.mc2_spell_mana_cost_at(hx, spell, t, castle);
        let e = &mut self.g.ent[m];
        e.f71 = t as u8;
        e.f30 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
        e.f28 = sub.word_0x18.max(0) as u16;
        // `byte_0x3B_59 = (fontType_0x1B & 1) == 0` (L:1519) — THE
        // per-tier cadence flag: 1 = CLICK-to-fire, 0 = RAPID
        // (auto-repeat while held; in the CD table only fireball
        // tier 1 "Repeat Fireball" and lightning tier 0 —
        // docs/traces/mc2-cast-input.md §2).
        e.f59 = (sub.font_type & 1 == 0) as u8;
        e.f136 = sub.max_mana_limit;
        e.max_life = cost.max(0) as u32;
        e.f140 = if e.f28 != 0 {
            cost / e.f28 as i32
        } else {
            cost
        };
        if self.dev_spells || self.mc2_free_spells {
            // Retail's OWN cheat flag (`OptionsSettingFlag_24 & 0x20`,
            // L:1531-35): no castle-upkeep gate, 1 mana per tick.
            // `mc2_free_spells` IS that flag, replayed off a recorded
            // cheat; the dev-spells instrument mirrors it so upkeep
            // spells (Cave-In's 100k castle-pool gate) stay castable.
            let e = &mut self.g.ent[m];
            e.f136 = 0;
            e.f140 = 1;
        }
    }

    /// `sub_6D9C0` (EF:43873) — the single-player level law: level =
    /// the highest tier (scanning down from the row's tier count)
    /// whose `xpos1_E` the effective XP (banked + volatile) reaches.
    /// Also: the castle XP hard-cap at 7 (EF:43885), the selected-
    /// tier clamp, the notification (msg 159/160+idx → sound 61,
    /// `sub_6DC40_improve_ability` EF:44007), and the optional bank
    /// commit capped at tier-2's threshold.
    pub(crate) fn mc2_relevel(&mut self, hx: Hx, spell: usize, bank: bool, notify: bool) {
        let Some(row) = self.g.assets.spells.get(spell).copied() else {
            return;
        };
        let owned = self.mc2_book[hx].ent[spell] != 0;
        // Retail's v5 gate (EF:43876-78): `(array_0x3E9 ||
        // SpellsEnabled) && (isCaveLevel || spell != 25)` — the port
        // unifies grant + manifestation into `ent` (the OR collapses;
        // no path sets one without the other), so v5 = owned + the
        // CAVE gate: Cave-In (25) never notifies or banks on a
        // surface level. The LEVEL derive stays unconditional.
        let v5 = owned && (self.g.is_cave() || spell != 25);
        // Castle XP clamp (EF:43885-86). The `setting_byte2_23 >= 0`
        // guard is CLOSED (2026-08-22, mc2l0-test): that word is
        // retail's tester flag, so the clamp is exactly what a normal
        // campaign does and a cheat-enabled session does NOT. Measured:
        // under the cheat menu the castle's volatile XP runs free to
        // 7900 and `levels[2]` reaches 2 on the very first spell-XP
        // press — clamping there would peg the castle at tier 0/1 and
        // silently break every cheated take's castle ladder.
        // `cheat_mode` is the flag, inferred from a cheat having fired
        // (engine::world::cheats).
        if !self.cheat_mode && self.mc2_book[hx].xp_vol[2] > 7 {
            self.mc2_book[hx].xp_vol[2] = 7;
        }
        let xp = self.mc2_book[hx].xp_vol[spell] + self.mc2_book[hx].xp_bank[spell];
        let mut v6 = row.byte_0 as i32;
        loop {
            v6 -= 1;
            if v6 < 0 || xp >= row.tiers[(v6 as usize).min(2)].xpos1 {
                break;
            }
        }
        let lvl = v6.max(0) as u8;
        if lvl != self.mc2_book[hx].levels[spell] {
            self.mc2_book[hx].levels[spell] = lvl;
            if v5 && notify {
                // `sub_6DC40_improve_ability`: on-screen message
                // (string 159 + 160+idx) + sound 61.
                let msg = self.mc2_relevel_message(spell);
                self.set_seat_notification(hx, msg, NOTIFY_TICKS_LEVELUP, NOTIFY_RED);
                self.g.snd_player(hx, 61);
            }
        }
        // Under the all-spells instrument the selection cap is the
        // tier count, not the XP level — so a mid-play XP award must
        // not yank a dev-selected high tier back down (it would desync
        // the pane and the next commit).
        let sel_cap = if self.dev_spells {
            (row.byte_0 as i16 - 1).clamp(0, 255) as u8
        } else {
            self.mc2_book[hx].levels[spell]
        };
        if self.mc2_book[hx].sel[spell] > sel_cap {
            self.mc2_book[hx].sel[spell] = sel_cap;
        }
        if v5 && bank {
            let cap = row.tiers[2].xpos1;
            self.mc2_book[hx].xp_bank[spell] = xp.min(cap);
        }
    }

    /// `sub_6D8B0` (EF:58228) — the XP award: `amount` onto the
    /// volatile XP of the owner's spell. Retail's own guard is
    /// `class == 3 && model == 0` — the HUMAN wizard ONLY
    /// (EF:58240-41). Rival owners
    /// are a structural no-op: retail rivals have NO spell-XP
    /// progression — their tiers are the authored map levels for
    /// life, and the tier-down walk supplies the dynamics. The
    /// castle arm re-syncs the manifestation tier; the level
    /// re-derives (per-award calls never bank, `a4=0`).
    pub(crate) fn mc2_award_xp(&mut self, owner: u16, spell: usize, amount: i32) {
        if spell >= 26 {
            return;
        }
        let Some(hx) = self.hx_of_tag(owner) else {
            return; // the model-0 guard — rivals never accrue
        };
        // THE CASTLE-LEVEL-UP LATCH CLEAR (`sub_60480` EF:61593).
        // Retail's clear and this award sit three lines apart, and the
        // ONLY producer of a `(PLAYER_TARGET, 2, 1)` award is
        // `Gen::mc2_castle_upgrade`'s own `sub_6D8B0` mail
        // (mc2/castle.rs:302) — a DOWNGRADE awards nothing, and no
        // projectile can forge it: `f40` is stamped from the spell
        // index by `mc2_launch` alone, which spell 2 never reaches
        // (the castle dispatches to `cast_castle`). So the join is
        // exact. ABOVE the life gate on purpose — retail's clear is
        // unconditional, and a castle finishing its level-up while the
        // wizard is dead must not strand the latch set forever.
        if spell == 2 && amount == 1 {
            self.mc2_recast_surcharge[hx] = false;
        }
        // THE CAMPAIGN-REPLAY XP GATE (`setting_38545 & 4`) —
        // `sub_6D8B0`'s own first line (EF:58596; NETHERW.EXE file
        // 0x920C1 `testb $0x4,0x9691(%eax)` / `jne` the epilogue).
        // A level this save has ALREADY COMPLETED accrues no spell XP
        // at all. BELOW the surcharge clear on purpose: that clear is
        // `sub_60480`'s own `byte_0x1BE_446 = 0` (EF:61992), three
        // lines ABOVE its `sub_6D8B0` call and outside the gate.
        // See [`no_mc2_replay_xp_gate`].
        if self.mc2_level_replayed && !no_mc2_replay_xp_gate() {
            return;
        }
        // ⛔ THE LIFE GUARD THAT USED TO SIT HERE WAS INVENTED —
        // `sub_6D8B0` has no life or state test anywhere in its 264
        // shipped bytes (0x920B0..0x921B7); its only gates are the
        // replayed-level bit, `owner != 0`, `class3f == 3` and
        // `model40 == 0`. See [`no_mc2_xp_award_while_dead`] for the
        // disassembly and the mc2l24-crazy t=3209 witness.
        if no_mc2_xp_award_while_dead() && self.human_state(hx) != LifeState::Alive {
            return;
        }
        self.mc2_book[hx].xp_vol[spell] += amount;
        if spell == 2 {
            let m = self.mc2_book[hx].ent[2] as usize;
            if m != 0 {
                // `sub_6D8B0`'s own SetSpell, UNsuppressed — a
                // short-arm no-op while the cast window is open; the
                // suppressed mid-transform re-sync is the LADDER
                // stamp's (`sub_60780` → the mc2_ladder_sync mail,
                // drained just before this one).
                self.mc2_set_spell(hx, m, self.mc2_book[hx].sel[2]);
            }
        }
        self.mc2_relevel(hx, spell, false, true);
    }

    /// The pane/book view (the CTRL pane + HUD consume this): per
    /// spell — owned, level, selected tier, effective XP; plus the
    /// quick-slot bindings. Read-only snapshot.
    pub fn mc2_book_view(&self, hx: Hx) -> Mc2BookView {
        let mut owned = [false; 26];
        let mut xp = [0i32; 26];
        let mut xpos = [[0i32; 3]; 26];
        let mut cost = [0u32; 26];
        let mut cost_tier = [[0u32; 3]; 26];
        let mut armed = [false; 26];
        let mut expiring = [false; 26];
        let mut castable = [[false; 3]; 26];
        // Retail's canSummon castle-pool probe (EF:22504-05): the own
        // castle's STORED mana, resolved once for the whole pane.
        let castle_mana = self.seat_castle(hx).map_or(0, |c| self.g.ent[c].f140);
        for s in 0..26 {
            owned[s] = self.mc2_book[hx].ent[s] != 0;
            xp[s] = self.mc2_book[hx].xp_vol[s] + self.mc2_book[hx].xp_bank[s];
            let tier = (self.mc2_book[hx].sel[s] as usize).min(2);
            if let Some(row) = self.g.assets.spells.get(s) {
                for t in 0..3 {
                    xpos[s][t] = row.tiers[t].xpos1;
                    // `canSummon`/`canSubSummon` (EF:22503-08 /
                    // EF:22602-08): the tier's `maxManaLimit_A` is
                    // zero, or the castle pool covers it (no castle ⇒
                    // any nonzero requirement greys). Read from the
                    // SPELLS table, not the manifestation — the dev
                    // instrument zeroes the manifestation's copy.
                    let mml = row.tiers[t].max_mana_limit;
                    castable[s][t] = mml <= 0 || castle_mana as i64 >= mml as i64;
                }
            }
            cost[s] = self.mc2_spell_mana_cost(hx, s, tier).max(0) as u32;
            for t in 0..3 {
                cost_tier[s][t] = self.mc2_spell_mana_cost(hx, s, t).max(0) as u32;
            }
            let m = self.mc2_book[hx].ent[s] as usize;
            armed[s] = m != 0 && self.g.ent[m].f26 > 0;
            // The blink set is retail's `isEnabled_1 & 4`, stamped in
            // CODE, not the SPELLS file (SetDefaultSpells_5C0A0
            // Spells.cpp:122-30): Speed Up/Morph/Shield/Rebound/
            // Invisible/Beyond Sight/Duel. Threshold 32, half MC1's.
            expiring[s] = m != 0
                && matches!(s, 3 | 4 | 6 | 8 | 11 | 12 | 14)
                && (1..32).contains(&self.g.ent[m].f26);
        }
        Mc2BookView {
            owned,
            levels: self.mc2_book[hx].levels,
            sel: self.mc2_book[hx].sel,
            xp,
            xpos,
            cost,
            cost_tier,
            armed,
            expiring,
            castable,
            left: self.mc2_book[hx].left,
            right: self.mc2_book[hx].right,
            ring: self.mc2_book[hx].ring,
            cave: self.g.is_cave(),
        }
    }

    // ---- selection (the 0x1F/0x20 "Change Spell" handler) ----------------

    /// EF:37898-37928: persist the chosen tier, bind the quick-slot,
    /// apply via SetSpell, sound 14. `hand`: 0 = left, 1 = right.
    /// The select-time hint text (`hintText_0x16x`) is the app's
    /// concern (it owns the notification surface).
    /// ⭐⭐ **THE TWO HANDS ARE ONE LANE, AND EVERY WRITER OWES THE
    /// MIRROR.** `SpellIndexLeft_0x451_1105` / `..Right_0x453_1107`
    /// live once in retail; the port keeps the cast machine's copy in
    /// `mc2_book` and the import/obs copy in `Player`, so a writer
    /// that touches only the book projects a STALE hand. The equip
    /// path learned this in 2026-08-22 (mc2l3 t=108) and got the
    /// mirror inline; the other five writers did not, and the pickup
    /// was the one the corpus caught next — mc2l0 t=3919, the scroll
    /// at slot 114 binds spell 2 into a left hand whose book says 2
    /// and whose obs still said 0. Route every hand write here.
    pub(crate) fn mc2_set_hand(&mut self, hx: Hx, right: bool, spell: i8) {
        let mirror = (spell >= 0).then_some(crate::mc1::spells::SpellId(spell as u8));
        if right {
            self.mc2_book[hx].right = spell;
            self.players[hx].right = mirror;
        } else {
            self.mc2_book[hx].left = spell;
            self.players[hx].left = mirror;
        }
    }

    pub fn mc2_select_spell(&mut self, hx: Hx, spell: u8, tier: u8, hand: u8) {
        let s = spell as usize;
        if s >= 26 {
            // Unbind semantic (spell out of range clears the hand —
            // the pane's empty-slot commit).
            self.mc2_set_hand(hx, hand != 0, -1);
            return;
        }
        // Cave-In is refused off-cave before anything else
        // (PlayerInput.cpp:849: `!SpellEnabled[s] || !isCaveLevel &&
        // s == 25` skips the whole select) — the pane draws it as the
        // plain box there (`Mc2BookView::cave`).
        if s == 25 && !self.g.is_cave() {
            return;
        }
        // Dev instrument: selecting an unowned spell under the
        // all-spells toggle self-grants the manifestation.
        if self.mc2_book[hx].ent[s] == 0 && self.dev_spells {
            self.mc2_dev_grant(hx, s);
            // Marked for the toggle-off release (`set_dev_spells`).
            if self.mc2_book[hx].ent[s] != 0 {
                self.dev_minted |= 1 << s;
            }
        }
        if self.mc2_book[hx].ent[s] == 0 {
            return;
        }
        // Normally the selectable tier is capped at the XP-earned
        // level; the all-spells (G) instrument keeps EVERY tier
        // exercisable, matching the app's pane (main.rs:2035). Without
        // the dev arm the sim casts tier 0 while the selector shows
        // tier N.
        let cap = if self.dev_spells {
            self.g
                .assets
                .spells
                .get(s)
                .map_or(0, |r| (r.byte_0 as i16 - 1).clamp(0, 255) as u8)
        } else {
            self.mc2_book[hx].levels[s]
        };
        let t = tier.min(cap);
        self.mc2_book[hx].sel[s] = t;
        // Retail's SpellIndexLeft/Right is ONE lane; the port splits
        // it into the book (the cast machine's read) and the Player
        // mirror (the import/obs lane) — keep both in step, or an
        // equip pair's obs projects the stale hand (mc2l3 t=108).
        self.mc2_set_hand(hx, hand != 0, spell as i8);
        let m = self.mc2_book[hx].ent[s] as usize;
        self.mc2_set_spell(hx, m, t);
        // ⭐⭐⭐ ARMING TELEPORT II IS WHAT SETS THE RETURN POINT.
        // `CopyAxisForSpellWithLife_6D830` (EF:58203-12) is called by
        // this very handler ONE STATEMENT AFTER `SetSpell_6D5E0`
        // (EF:37921-22):
        //   if (entity->model_0x40_64 == 10
        //       && Entities[entity->parentId] > Entities[0]
        //       && SPELLS[10].subspell[spellId].life_0x1A == 1)
        //       entity->axis_0x9A_154x = Entities[parentId]->position;
        // so the T1 save/return toggle's anchor is stamped at EQUIP
        // time, not at the castle hop. mc2l22 t=13242: the human equips
        // Teleport II and retail rewrites token 219's dest_x/y/z
        // 34970/64159/5523 -> 32979/50259/6008 — the carpet's position
        // at the top of that tick — while `word_0x96_150` stays 1;
        // seven ticks later the cast restores exactly that axis.
        // ⚠ The two halves are NOT separable: the return-leg read alone
        // adds a head at 16242 because the imported axis goes stale
        // without this stamp. One switch covers both.
        if !crate::engine::world::mc2_teleport_save_axis_off()
            && m != 0
            && self.g.ent[m].model65 == 10
            && self
                .g
                .assets
                .spells
                .get(10)
                .is_some_and(|r| r.tiers[(t as usize).min(2)].life == 1)
        {
            let (px, py, pz) = self.human_pose(hx);
            let e = &mut self.g.ent[m];
            e.dest_x = px;
            e.dest_y = py;
            e.site_z = pz;
        }
        self.g.snd_player(hx, 14);
        // The change-spell toast (EF:37925), the shared all-games law
        // (`World::spell_select_toast`): MC2 supplies the chosen TIER's
        // own name ("Possession" / "Mana Magnet" / "Thunderstorm"), so
        // a level-N pick reads as its distinct spell.
        let name = self.mc2_spell_name(s, t as usize);
        self.spell_select_toast(hx, &[(hand != 0, name)]);
    }

    /// Retail cmd 0x26 (SHIFT+click fast-bind, EF:37950-53): a raw
    /// byte store into the cycle ring + sound 14. No equip
    /// side-effect — ring membership is a separate concept from what
    /// sits on each button. The toggle/move truth table lives in the
    /// SENDER (the app's pane click, PI:856-878); the sim just
    /// stores.
    pub(crate) fn mc2_ring_set(&mut self, hx: Hx, spell: u8, val: u8) {
        let s = spell as usize;
        if s >= 26 {
            return;
        }
        self.mc2_book[hx].ring[s] = val.min(2);
        self.g.snd_player(hx, 14);
    }

    /// The rest of retail's cross-level carry beyond what
    /// `mc2_grant_plausible` re-derives: the cycle ring
    /// (`array_0x3B5`), carried RAW — even for spells not possessed
    /// this level.
    ///
    /// ⚠ NOT the selected tiers and NOT the hands. `sub_549A0` does
    /// copy `array_0x437` (the per-spell tier) into the next level, but
    /// the level start `sub_53160` then runs `InitialiseSpells_54A50`,
    /// which zeroes every tier (EF:38952 — every spell starts the level
    /// at tier 1) and sets both hands to −1, re-binding them to the
    /// first two spells it grants (EF:39048-56) — the canonical binding
    /// the grant pass already makes. The port carried both, citing the
    /// SAVE-GAME load path (Level.cpp `LoadLevelSLEV` → `sub_549A0`),
    /// which restores a mid-level save exactly; corrected 2026-11-15
    /// (player-reported: a level opening in danger with a high tier
    /// still selected).
    pub fn mc2_install_ring_carry(&mut self, hx: Hx, ring: &[u8; 26]) {
        if !matches!(self.game(), crate::ids::GameId::Mc2) {
            return;
        }
        for s in 0..26 {
            self.mc2_book[hx].ring[s] = ring[s].min(2);
        }
    }

    /// The dev-toggle grant: a manifestation with no jar (state 3M,
    /// hidden by the draw filter), wired like a pickup.
    /// Test hook: grant one spell through the PICKUP path, so a test
    /// can pin that the jar law still overwrites the left hand.
    #[cfg(test)]
    pub(crate) fn mc2_dev_grant_for_test(&mut self, spell: usize) {
        self.mc2_dev_grant(Hx::FIRST, spell);
    }

    fn mc2_dev_grant(&mut self, hx: Hx, spell: usize) {
        let (px, py, pz) = self.human_pose(hx);
        if let Some(m) = self.mc2_new_spell_token(spell as u8, px, py, pz) {
            self.mc2_adopt_manifestation(hx, m, spell);
        }
    }

    /// Install a plausible MC2 spellbook — the MC2 arm of the
    /// `plausible_spellbook` playtest instrument (MC1's lives in
    /// `campaign::plausible_spellbook` + `grant_spells`). For each
    /// `(spell, banked_xp)`: learn the spell if unowned (a hidden
    /// manifestation like the dev grant) and set its BANKED
    /// (campaign-carried) XP, then re-derive the tier from the SPELLS
    /// `xpos1` ladder — the same thresholds a real playthrough crosses,
    /// so a plausible scroll count yields a plausible tier. `banked_xp`
    /// is the app's campaign estimate (jar union → learned set; scroll
    /// census → XP). No-op off-MC2 (the book is MC2-only state).
    pub fn mc2_grant_plausible(&mut self, hx: Hx, grants: &[(u8, i32)]) {
        if !matches!(self.game(), crate::ids::GameId::Mc2) {
            return;
        }
        for &(spell, xp) in grants {
            let s = spell as usize;
            if s >= 26 {
                continue;
            }
            if self.mc2_book[hx].ent[s] == 0 {
                self.mc2_dev_grant(hx, s);
            }
            // Grant can fail if the event pool is exhausted; skip XP.
            if self.mc2_book[hx].ent[s] == 0 {
                continue;
            }
            self.mc2_book[hx].xp_bank[s] = xp.max(0);
            // Derive the level from the new banked XP (no re-bank, no
            // level-up toast — this is an init-time install).
            self.mc2_relevel(hx, s, false, false);
        }
        // Level-start binding, not the pickup law (see the fn doc).
        self.mc2_rebind_hands_canonical(hx);
    }

    /// The carried spells this level BLOCKS for the human
    /// (`BlockedSpells_0x36115x`): retail's level-start grant skips
    /// them (`InitialiseSpells_54A50` EF:39014 — the carry is consulted
    /// only `if (!BlockedSpells[s])`), yet they stay KNOWN
    /// (`array_0x3E9`, never cleared) with their XP intact, so the
    /// level's own jar hands them back at their old tier. mc2:017's
    /// Alliance quest jar is the case the player meets. For each
    /// `(spell, banked_xp)` not already owned: bank the XP and derive
    /// the tier, minting nothing. No-op off-MC2.
    pub fn mc2_bank_withheld(&mut self, hx: Hx, withheld: &[(u8, i32)]) {
        if !matches!(self.game(), crate::ids::GameId::Mc2) {
            return;
        }
        for &(spell, xp) in withheld {
            let s = spell as usize;
            if s >= 26 || self.mc2_book[hx].ent[s] != 0 {
                continue;
            }
            self.mc2_book[hx].xp_bank[s] = xp.max(0);
            self.mc2_relevel(hx, s, false, false);
        }
    }

    /// The collect wiring shared by the jar pickup and the dev grant
    /// (token trace §3, EF:55715-49): the token BECOMES the wizard's
    /// spell object — state 3M, cooldown 64, owner rebound; grant +
    /// quick-slot bind + SetSpell at the chosen tier.
    pub(crate) fn mc2_adopt_manifestation(&mut self, hx: Hx, m: usize, spell: usize) {
        {
            let e = &mut self.g.ent[m];
            e.tick70 = (spell as u8).wrapping_mul(3);
            // ⚠ `word_0x36_54 = 64` — the 64-tick RE-STEAL LOCK — is
            // `sub_68FF0`'s COLLECT store (EF:56076, NETHERW.EXE
            // 0x8D93C), not part of the shared grant wiring: the reify
            // that hands a wizard its level-start book (`sub_55AB0`,
            // Level.cpp:1313) never touches @0x36. The pickup caller
            // arms it itself; see
            // [`crate::engine::features::no_mc2_pickup_only_steal_lock`].
            if crate::engine::features::no_mc2_pickup_only_steal_lock() {
                e.f54 = 64;
            }
            e.id24 = self.g.human_tag[hx].0;
            // ⚠ `e.f26 = 0; e.f44 = 0;` WERE INVENTED. `sub_68FF0`'s
            // collect block (EF:56056-64) writes exactly
            // `byte[0] |= 1; byte[3] &= 0xFD; parentId_0x28_40 = wiz;
            //  actionIndex_0x45_69 = a3; word_0x36_54 = 64;
            //  SpellEnabled[a2] = slot; array_0x403[a2] = 1` and then
            // the hand hint — it touches NEITHER `word_0x2E_46` NOR
            // `word_0x2C_44`. On a FRESH jar both are already 0, which
            // is why the invention was invisible; on a RE-COLLECTED
            // stolen jar it is the difference between a live cast
            // window and a dead one. WITNESS mc2l7 t=21683, slot 44
            // (the (15,9) meteor, re-collected one tick after its arc
            // lands): retail keeps `word_0x2E_46 = 2` and the
            // `mc2_set_spell` below then takes its SHORT ARM (`f26 > 0`
            // ⇒ park the tier in `word_0x2C_44`), which is why retail's
            // `f2c` goes 0 → 1 on that very tick. The port zeroed the
            // timer, SetSpell applied immediately, and `f2c` stayed 0 —
            // both halves of retail's row, from one invented store.
            if no_mc2_stolen_arc_keeps_cast_state() {
                e.f26 = 0;
            }
            // ⚠ `e.f44 = 0` IS ALSO AN INVENTION and is KEPT anyway.
            // Retail leaves @0x2C alone here too, but in the port that
            // word carries SLOT-RECYCLING RESIDUE into a fresh jar:
            // `MGC_WRITE_TRACE=11:f44` on mc2l7 names the writer as
            // **slot 25, an (11,36), at t=1257** — while slot 11 was
            // still its PREVIOUS tenant — and the class-15 ctor never
            // clears it. Drop this scrub and `sub_6D880` reads the
            // leftover 100 as "pending tier 99" the tick the (15,5)
            // heal window expires, stamping tier 2: mc2l7 t=1664,
            // `mana_max` 500 → 50000, `byte_0x46_70` 0 → 2. Dropping
            // the store therefore needs the ctor's @0x2C clear found
            // first; it is a separate lead, not part of this law. Harmless for the
            // law's own witness: `mc2_set_spell` two lines below
            // re-stamps `f44 = tier + 1` through its short arm whenever
            // the cast timer survived, which is exactly retail's
            // `f2c 0 → 1` at mc2l7 t=21683.
            e.f44 = 0;
        }
        self.mc2_book[hx].ent[spell] = m as u16;
        // The stolen-jar hand hint (`word_0x4A_74` → f36, sub_68FF0
        // EF:55728-40): 1 = re-equip the RIGHT hand, 2 = the LEFT —
        // the hand the wraith yanked it from; cleared after use.
        // Without a hint (fresh jars, dev grants) the quick-slot v12
        // law applies: left if free (or both taken), else right
        // (EF:55735-49).
        let hint = self.g.ent[m].f36;
        self.g.ent[m].f36 = 0;
        let to_left = if hint == 2 {
            true
        } else if hint == 1 {
            false
        } else {
            self.mc2_book[hx].left == -1 || self.mc2_book[hx].right != -1
        };
        self.mc2_set_hand(hx, !to_left, spell as i8);
        self.mc2_relevel(hx, spell, false, false);
        self.mc2_set_spell(hx, m, self.mc2_book[hx].sel[spell]);
    }

    /// `sub_61050`'s TIER FORK (EF:62130-62199) — the `life_0x1A`
    /// switch the port had never taken, on BOTH victim columns.
    ///
    /// ```text
    ///   v4 = SPELLS[13].subspell[tier].life_0x1A;        // 0, 0, 1 shipped
    ///   if (!v4)         { v33 = subSpellIndex_2; v35 = 1; }        // FLAT
    ///   else if (v4 > 2u){ goto LABEL_23; }                         // v33 = 0
    ///   else {
    ///     v6x = Entities[a1x->dword_0xA4_164x->CastleEntityIndex];  // VICTIM's
    ///     v7x = Entities[v34x->dword_0xA4_164x->CastleEntityIndex]; // CASTER's
    ///     if (v6x <= Entities[0] || v7x <= Entities[0] || v6x->mana <= 0)
    ///          v35 = 1;                       // → the FLAT arm, amount = pct
    ///     else { v9 = v6x->mana * pct;
    ///            v6x->mana -= v6x->mana * pct / 100;     // the VICTIM's CASTLE
    ///            v33 = v9 / 100;                          // …scattered, not paid
    ///            while (v33 > 0) { chunk = min(v33, 500); … } }
    ///     if (!v35) goto LABEL_23;            // v33 == 0 — NOTHING moves
    ///   }
    /// ```
    ///
    /// Returns the amount that moves WIZARD-TO-WIZARD at LABEL_23
    /// (EF:62202-09). ⭐⭐⭐ **THE CASTER IS NOT CREDITED THE SAME
    /// AMOUNT — IT IS NOT CREDITED AT ALL.** The tier-3 arm takes a
    /// PERCENTAGE off the VICTIM'S CASTLE, leaves both wizards' purses
    /// alone, and re-emits the stolen store as loose (10,39) spheres in
    /// a ring around the CASTER'S OWN castle; the thief has to fly out
    /// and collect them. The port ran retail's NO-CASTLE FALLBACK for
    /// tier 2 in every case, i.e. moved a flat 10 (the percent read as
    /// an amount) and never touched a castle.
    ///
    /// mc2l6-rsg t=19303→19304 is the corpus's ONE tier-2 steal (the
    /// only (10,25) burst in 40,422 ticks with `b46 == 2`; the other
    /// 172 rows are tiers 0/1). The human (343) steals from rival 378:
    /// castle 466's store **8580 → 7722** (= 8580 − 8580·10/100 = 858),
    /// the human's purse UNCHANGED at 1,449,370, 378's purse on its own
    /// −100 rhythm, and TWO (10,39) spheres born the same tick carrying
    /// **500 and 358** (= 858 in 500-chunks) at ~9,592 units from castle
    /// 63 — the caster's castle, whose `array_0x52_82` extents are
    /// (6784, 6784) and `isqrt(6784² + 6784²) = 9594`. Both spheres
    /// carry `playerEntityIndex = 0`, which is `life_0x1A == 1`'s arm
    /// (EF:62185-86) — tier 3 of spell 13 is NOT the caster-owned `== 2`
    /// case, so nothing about them is bound to the thief.
    pub(crate) fn mc2_steal_resolve(&mut self, victim: u16, caster: u16, tier: u8) -> u32 {
        let row = self.g.assets.spells.get(13).copied().unwrap_or_default();
        let sub = row.tiers[(tier as usize).min(2)];
        let flat = sub.sub_spell.max(0) as u32;
        // `if (v4 > 2u) goto LABEL_23` — v35 stays 0 AND v33 stays 0,
        // so nothing moves at all (unreachable on the shipped table).
        if !(0..=2).contains(&sub.life) {
            return 0;
        }
        if sub.life == 0 {
            return flat; // tiers 1/2: the flat 2000 / 4000
        }
        // EF:62135-38 — BOTH castles must resolve and the VICTIM's
        // store must be POSITIVE, else `v35 = 1` and the flat arm runs
        // with `subSpellIndex_2` (10 on tier 3) read as an AMOUNT.
        // `rival_castle` is the shared owner scan — PLAYER_TARGET is
        // the human's own `id24`, so it serves both columns.
        let (Some(vc), Some(cc)) = (self.rival_castle(victim), self.rival_castle(caster)) else {
            return flat;
        };
        let store = self.g.ent[vc].f140;
        if store <= 0 {
            return flat;
        }
        // EF:62144-48 — the product is 64-bit and the quotient
        // truncates; the castle keeps `store - store*pct/100`.
        let pct = sub.sub_spell as i64;
        let taken = (store as i64 * pct / 100) as i32;
        self.g.ent[vc].f140 = store - taken;
        // EF:62147/62149 — the emission RING is the CASTER's castle's
        // horizontal extents, `radix_3d(pitch² + roll²)`.
        let (cx, cy, radius) = {
            let e = &self.g.ent[cc];
            let (ap, ar) = (e.f80 as i32, e.f82 as i32);
            (e.x, e.y, Gen::isqrt((ap * ap + ar * ar) as u32) as i16)
        };
        // EF:62183-86 — only `life_0x1A == 2` binds the sphere to the
        // thief; tier 3 of spell 13 is 1, so the spheres are UNOWNED.
        let owner = if sub.life == 2 { caster } else { 0 };
        // The victim's OWN entity LCG carries the ring draw (`a1x` is
        // the victim), EF:62158-59. The human's carpet is a pool record
        // under conformance; native play has none, and the fallback
        // keeps the scatter random rather than degenerate.
        let vslot = if let Some(hx) = self.hx_of_tag(victim) {
            self.mc2_carpet_slot[hx] as usize
        } else {
            victim as usize
        };
        let mut left = taken;
        while left > 0 {
            // EF:62152-56 — at most 500 per sphere, remainder last.
            let chunk = left.min(500);
            left -= chunk;
            let ring_yaw = if vslot != 0 && vslot < self.g.ent.len() {
                (self.g.ent_rand(vslot) & 0x7FF) as u16
            } else {
                self.g.rand = self.g.rand.wrapping_mul(9377).wrapping_add(9439);
                (self.g.rand & 0x7FF) as u16
            };
            let mut pos = (cx, cy, 0i16);
            Gen::polar_step(&mut pos, ring_yaw, 0, radius);
            // EF:62163-65 — `HIBYTE(alt) += 4`, i.e. ground + 1024.
            pos.2 = (self.g.ground_z(pos.0, pos.1) as i16).wrapping_add(4 << 8);
            let Some(sp) = self.g.mc2_spawn_mana_sphere(39, pos.0, pos.1, pos.2) else {
                continue;
            };
            // EF:62170-81 — TWO draws on the SPHERE's OWN LCG.
            let r1 = self.g.ent_rand(sp);
            let yaw = ring_yaw.wrapping_add((r1 % 0x71) as u16).wrapping_sub(56) & 0x7FF;
            let r2 = self.g.ent_rand(sp);
            let speed = ((r2 % 0x30) + 16) as i16;
            let mut vel = (0u16, 0u16, 0i16);
            Gen::polar_step(&mut vel, yaw, 0, speed);
            let e = &mut self.g.ent[sp];
            e.f30 = yaw;
            e.f126 = speed;
            e.dest_x = vel.0;
            e.dest_y = vel.1;
            e.f46 = 128; // word_0x2C_44 (EF:62180 — the ctor's own value)
            e.f140 = chunk;
            e.f144 = owner;
        }
        0
    }

    /// `sub_69300` (EF:55792-826) — the m26 wraith SPELL-STEAL: yank
    /// the equipped jar out of the given hand (1 = right, 2 = left,
    /// the roll's 4/5 in [`Gen::m26_tick`]). The empty-hand and
    /// slot-0 aborts (EF:19354-58/19366-70) and the `word_0x36_54`
    /// re-steal lock (EF:55800) all run AFTER the %63 draw — a
    /// locked or empty-handed roll is simply spent. Jar-entity field
    /// homes: f38 = the wraith (`word_0x26_38`), f26 = the arc
    /// counter (`dword_0x10_16`), f36 = the hand hint
    /// (`word_0x4A_74`), tick70 = 78 (the shared class-15 detach
    /// action). The per-spell tier (`array_0x437` → sel) is NOT
    /// touched — XP survives the theft. Retail's `byte[0] &= ~1`
    /// in-hand bit is write-only in the port (its lone retail reader
    /// is the presentation-side owned-jar tint, unmodeled).
    pub(crate) fn mc2_spell_steal(&mut self, wraith: u16, hand: u8) {
        // The victim is the wraith's own target (`+146`, the drain lock
        // the steal roll ran under).
        let hx = self
            .g
            .ent
            .get(wraith as usize)
            .and_then(|e| self.hx_of_tag(e.f146))
            .unwrap_or(Hx::FIRST);
        let spell = if hand == 1 {
            self.mc2_book[hx].right
        } else {
            self.mc2_book[hx].left
        };
        if spell < 0 {
            return;
        }
        let s = spell as usize;
        let m = self.mc2_book[hx].ent[s] as usize;
        if m == 0 {
            return;
        }
        if self.g.ent[m].f54 != 0 {
            return; // the 64-tick re-steal lock
        }
        {
            let e = &mut self.g.ent[m];
            // ⭐ @0x26, NOT @0x24. `sub_69300`'s first store is
            // `mov %ax,0x26(%ebx)` (`NETHERW.EXE` 0x8DB32) and
            // `sub_59DC0`'s homing arm reads the wraith back from
            // `0x26(%ebx)` (0x7E656). The MC2 field map homes retail
            // @0x26 in `f40` (`import_ent_mc2`: `f40: tr(r.f26)`;
            // `port_ent_lanes_mc2`: the "f26" lane publishes `e.f40`)
            // — `f38` is @0x24, the killer latch. The old `f38` home
            // meant an IMPORTED mid-arc jar read wraith 0 and finished
            // on the spot.
            e.f40 = wraith;
            e.tick70 = 78;
            // ⭐ RETAIL ZEROES @0x10, NOT @0x2E. `sub_69300`
            // (EF:56136) is `a1x->dword_0x10_16 = 0`, and the shipped
            // EXE spells the width out — `NETHERW.EXE` file 0x8DB39-49
            // (linear 0x69339; MC2 file = VA + 0x24800):
            //   8db39: c6 43 45 4e          movb $0x4e,0x45(%ebx)  ; action 78
            //   8db49: c7 43 10 00 00 00 00 movl $0x0,0x10(%ebx)   ; @0x10 = 0
            // — a 32-bit store to @0x10 and **nothing anywhere in
            // `sub_69300` touches @0x2E**. The armed cast timer
            // survives the theft.
            if no_mc2_stolen_arc_keeps_cast_state() {
                e.f26 = 0;
            } else {
                e.f50 = 0;
            }
        }
        // Snap the jar onto the player (CopyEntityPosition, EF:55810).
        let (px, py, pz) = self.human_pose(hx);
        self.g.move_relink(m, px, py, pz);
        // Unlearn (`SpellEnabled[model] = 0`, EF:55811) — the pane
        // greys out and the ground jar becomes collectible again.
        self.mc2_book[hx].ent[s] = 0;
        self.g.mc2_spell_tokens[hx].0 &= !(1 << s);
        // Unequip every hand holding the model; the hint remembers
        // the LAST cleared hand (left wins on the both-hands edge,
        // EF:55814-24 — independent ifs, verbatim).
        self.g.ent[m].f36 = 0;
        if self.mc2_book[hx].right == spell {
            self.mc2_set_hand(hx, true, -1);
            self.g.ent[m].f36 = 1;
        }
        if self.mc2_book[hx].left == spell {
            self.mc2_set_hand(hx, false, -1);
            self.g.ent[m].f36 = 2;
        }
        self.entities_dirty = true;
    }

    // ---- the cast gate ----------------------------------------------------

    /// `sub_5F380`'s per-button dispatch (EF:60748) under the
    /// press/hold law (docs/traces/mc2-cast-input.md §1-2): the fire
    /// bits are EDGE-triggered per press
    /// (`HandleMouseButtons_18F80`, PI:2027-76); a HELD button
    /// re-fires only when the bound tier is RAPID (`byte_0x3B_59 !=
    /// 1`) and its cast window is live — that is the whole
    /// click-vs-Repeat-Fireball difference.
    ///
    /// The two retail registers behind `edge`/`held` are the two
    /// halves of `MouseButtonState_18059C`, rebuilt every poll at
    /// EF:49675-83: bit 0/1 = the ISR PRESS LATCH
    /// (`x_WORD_180746`/`180744`), bit 2/3 = the HELD state
    /// (`x_WORD_18074C`/`18074A`). `HandleMouseButtons_18F80` fires a
    /// non-rapid spell off bit 0 ALONE and clears it (PI:2043-49),
    /// and the frame tail clears the global latch whenever bit 0 is
    /// down (PI:1049-52) — hence exactly ONE cast per physical click,
    /// however long the button is held. Measured on mc2l4 0+4000: 409
    /// recorded press edges, 404 retail possession arms, and the port
    /// (once its edge lane was alive) 408.
    pub(crate) fn mc2_cast_input(
        &mut self,
        hx: Hx,
        edge: (bool, bool),
        held: (bool, bool),
        ring: Option<u8>,
    ) {
        if self.human_state(hx) != LifeState::Alive {
            return;
        }
        // ⭐⭐⭐ ROUND 98 — THE RECORDED MOVE BYTE IS ALREADY RETAIL'S
        // POST-LATCH COMMAND WORD. `sub_5F380`'s cast tail is three
        // flat `testb`/`call` pairs on `entityIndex_0x0` with NO edge
        // test at all (`NETHERW.EXE` 0x83D53-0x83DED — full citation
        // and the mc2l22 t=3424 witness on
        // [`crate::mc2::effects::no_mc2_command_word_cast`]), so under
        // the conformance seat the trigger is the RAW BIT and only
        // `mc2_cast_gate`'s per-model refusal may drop it. Re-deriving
        // a press edge from an already-latched word is double
        // latching, and it eats every press landing one frame after
        // another press.
        let word_cast = self.strict_retail && !crate::mc2::effects::no_mc2_command_word_cast();
        let fires = |w: &World, spell: i8, edge: bool, held: bool| {
            if spell < 0 {
                return false;
            }
            let m = w.mc2_book[hx].ent[spell as usize] as usize;
            if m == 0 {
                return false;
            }
            if word_cast {
                return held;
            }
            // `byte_0x3B_59 == 1` is the CLICK-ONLY family; every
            // other value takes the repeat arm (PI:2043 vs PI:2050 —
            // the test is `== 1`, not `!= 0`).
            edge || (held && w.g.ent[m].f59 != 1 && w.g.ent[m].f26 > 0)
        };
        if fires(self, self.mc2_book[hx].left, edge.0, held.0) {
            self.mc2_cast_gate(hx, self.mc2_book[hx].left as usize, false);
        }
        if fires(self, self.mc2_book[hx].right, edge.1, held.1) {
            self.mc2_cast_gate(hx, self.mc2_book[hx].right as usize, true);
        }
        // ⭐⭐⭐ THE THIRD ARM (EF:60858-62). `if (w & 0x40) sub_5F660(
        // a1x, SpellEnabled[spellIndex_D94FF[spellIndex_0x458_1112]],
        // 256)` — the cycle-ring SHORTCUT casts the spell under the
        // ring cursor WITHOUT equipping it, consulting neither hand,
        // and stamps the LEFT hand bit (256) like the left button.
        // `spellIndex_D94FF` (GameUI.cpp:59) is the identity over
        // 0..25, so the index IS the recorded `ring_cursor`. It is a
        // BARE `testb` on the same already-latched command word the
        // two hands ride, so there is no edge/repeat leg here:
        // `mc2_cast_gate` is the whole body, exactly as retail's third
        // `call` is. mc2l22 t=63318 records `move_bits = 64` outright
        // with both hands holding spells 9 and 1, and retail arms the
        // (15,20) Gravity Well token (`f2e` 0 -> 27) — the take's last
        // divergence head.
        if !crate::engine::world::no_mc2_ring_cast_bit()
            && let Some(spell) = ring
            && (spell as usize) < self.mc2_book[hx].ent.len()
        {
            self.mc2_cast_gate(hx, spell as usize, false);
        }
    }

    /// `sub_5F660` (EF:60874) — the cast gate: the per-model
    /// re-arm/retrigger switch, then the mana gate (`mana <
    /// maxMana` → fail sound 29), then the arm (`sub_5F7B0`
    /// EF:60973: timer = duration).
    fn mc2_cast_gate(&mut self, hx: Hx, spell: usize, right: bool) {
        let m = self.mc2_book[hx].ent[spell] as usize;
        if m == 0 {
            return;
        }
        // Cave-In is CAVE-ONLY: refused off-cave (EF:43883/48253,
        // PI:849; the icon grey-out EF:22470 is the UI's side).
        if spell == 25 && !self.g.is_cave() {
            return;
        }
        let (armed, tier) = (self.g.ent[m].f26, self.g.ent[m].f71);
        match self.g.ent[m].model65 {
            // Fireball: tier < 2 re-arms freely; the charged tier
            // refuses while airborne (EF:60895-98 → LABEL_16).
            0 => {
                if tier >= 2 && armed > 0 {
                    return;
                }
            }
            // Possess: an active cast is not re-armed/re-charged —
            // the marker timer never refreshes — but the re-press
            // raises the `byte_0x3C_60 = 1` RELEASE SIGNAL (→ f56),
            // records the firing hand, and runs the invis-break law,
            // IN THAT ORDER and with NO mana gate: retail's arm is
            // `byte_0x3C_60 = 1; byte[1] &= 0xFC; dword |= v3;
            // sub_5F7E0(); v7 = 1; goto LABEL_23` (EF:60900-07) —
            // LABEL_23 buzzes only on `v6`, which this path never
            // sets, so a broke wizard re-pressing possession gets the
            // signal and NO sound 29. The tier-0 consumer discards
            // the signal (`sub_69640` EF:56013); the higher tiers
            // spend it on a re-fire.
            1 if armed > 0 => {
                self.g.ent[m].f56 = 1;
                self.mc2_stamp_hand(hx, m, right);
                self.mc2_arm_invis_break(hx, spell);
                return;
            }
            // Castle: a re-cast while the ball flies buzzes
            // (EF:60908-13).
            2 if armed > 0 => {
                self.g.snd_player(hx, 29);
                return;
            }
            // Lightning: tier 0 re-arms freely (the RAPID stream);
            // tier 1+ refuses while armed (EF:60929-33).
            7 if tier >= 1 && armed > 0 => return,
            // The channel retriggers: an active cast is EXTENDED
            // (metamorph 7 ticks, the rest 1), no re-charge
            // (EF:60914-28).
            4 | 6 | 8 | 0xB | 0xC | 0xE if armed > 0 => {
                self.g.ent[m].f26 = if self.g.ent[m].model65 == 4 { 7 } else { 1 };
                return;
            }
            // The LABEL_16 band: no re-arm while active
            // (EF:60946-48).
            9 | 0xA | 0xD | 0xF | 0x10..=0x18 if armed > 0 => return,
            _ => {}
        }
        // THE MANA GATE (EF:60953): caster mana vs the tier's full
        // cost. Insufficient → UI flash + sound 29 (EF:60964-67).
        // Reads the PRE-apply purse — retail's gate tail (`sub_5F380`)
        // sits before the wizard body's mana block, and so does this
        // now that the block runs at the carpet's own dispatch, so
        // the LIVE purse is that value and the `mc2_gate_purse` stash
        // it used to need is gone. A dry-out tick still re-arms and
        // the manifestation's own post-apply first-tick re-check
        // collapses the window the same tick — regen resumes
        // immediately, not f26 ticks late.
        let cost = self.g.ent[m].max_life;
        if !self.dev_spells && (self.human_mana(hx) as u64) < cost as u64 {
            self.g.snd_player(hx, 29);
            return;
        }
        // `sub_5F7B0`: ARM — timer = duration; the effect state now
        // fires. A zero-duration row still casts for one tick. The
        // firing button is recorded ON THE CASTER, where retail puts
        // it (`byte[1] &= 0xFC; …dword |= a3`, a3 = 256/512,
        // EF:60973-82) — the launch reads it back exactly as
        // `sub_68E50` does.
        self.g.ent[m].f26 = self.g.ent[m].f28.max(1) as i16;
        self.mc2_stamp_hand(hx, m, right);
        // A release signal left over from the marker's last tick
        // must not refire into the fresh arm.
        self.g.ent[m].f56 = 0;
        self.mc2_arm_invis_break(hx, spell);
    }

    /// `sub_5F7B0`'s hand stamp (EF:60977-78):
    /// `caster->byte[1] &= 0xFC; caster->dword |= a3` with `a3` =
    /// 256 for the left button (EF:60852) / 512 for the right
    /// (EF:60855) — the SAME storage and the SAME constants as MC1's
    /// `:55894-95`, so the shared `hand_bits` register holds it and
    /// the conformance import can seed it off the recorded carpet.
    /// The `dual_wield_muzzle` patch additionally remembers the hand
    /// on the TOKEN (`World::token_hand`, port-only, both arms write).
    fn mc2_stamp_hand(&mut self, hx: Hx, m: usize, right: bool) {
        self.set_human_hands(
            hx,
            (self.human_hands(hx) & !0x300) | if right { 0x200 } else { 0x100 },
        );
        self.token_hand.insert(m as u16, right);
    }

    /// The Invisibility per-tier break-on-self-cast law (`sub_5F7E0`
    /// EF:60987, run from the arm path `sub_5F7B0` AND the possess
    /// re-press): arming ANY spell may break an active cloak.
    /// `s = byte_0x1BF_447` (invis strength): T0 (s=1) any cast
    /// breaks; T1 (s=2) breaks on everything except possess (spell
    /// 1); T2 (s=3) nothing breaks. The invis FIRST cast doesn't
    /// self-break — strength is still 0 here (set on the invis
    /// effect's first tick). On break we also zero the invis
    /// window's `f26` so the mana-regen block lifts with the cloak
    /// (functional termination must clear the burst).
    /// docs/spell-audit/rival-spells.md §2.
    fn mc2_arm_invis_break(&mut self, hx: Hx, spell: usize) {
        let s = self.players[hx].invis_strength;
        if s != 0 && (s < 2 || (s <= 2 && spell != 1)) {
            self.set_human_invisible(hx, false);
            self.players[hx].invis_strength = 0;
            let inv = self.mc2_book[hx].ent[0xB] as usize;
            if inv != 0 {
                self.g.ent[inv].f26 = 0;
            }
        }
    }

    // ---- the per-tick effect states ---------------------------------------

    /// `sub_68D50` (EF:55548) — may the cast proceed this tick?
    /// Caster alive; upkeep spells need the own castle's pool to
    /// cover `manaRegen`; the first tick re-checks the full cost.
    fn mc2_afford(&self, hx: Hx, m: usize) -> bool {
        if self.human_state(hx) != LifeState::Alive {
            return false;
        }
        let e = &self.g.ent[m];
        if e.f136 > 0 {
            let ok = self
                .seat_castle(hx)
                .is_some_and(|c| self.g.ent[c].f140 >= e.f136);
            if !ok {
                return false;
            }
        }
        // `.max(1)` mirrors the arm/first-tick sites — a zero-
        // duration row arms f26=1; comparing against a raw 0 would
        // skip the first-tick full-cost re-check.
        if e.f26 as u16 == e.f28.max(1) {
            return self.dev_spells || self.human_mana(hx) as u64 >= e.max_life as u64;
        }
        true
    }

    /// The canonical effect-state skeleton (cast-path trace §1.5),
    /// run for every learned manifestation each tick: while armed —
    /// afford-check, FIRST-tick spawn + mana commit (`sub_68DE0` =
    /// the negative-delta stamp, [`World::mana_debit`]), countdown,
    /// pending-tier apply at expiry (`sub_6D880`); plus the cooldown
    /// tick (`word_0x36_54--`).
    pub(crate) fn mc2_cast_tick(&mut self, hx: Hx, p: PlayerPose, ctx: &MobCtx) {
        for spell in 0..26usize {
            let m = self.mc2_book[hx].ent[spell] as usize;
            if m == 0 {
                continue;
            }
            // Retail runs the effect body as the class-15 entity's own
            // action 3M (3M+1/3M+2 are the pickup states, 78 the
            // wraith-steal arc), so the port's book-driven loop tests
            // for it — which also disambiguates the death scatter's
            // BOOLEAN 1 marker (`sub_5E310` EF:60146,
            // `mc2_scatter_spells`) from a real slot-1 manifestation.
            if self.g.ent.get(m).is_none_or(|e| {
                e.class64 != 15 || e.model65 as usize != spell || e.tick70 as usize != spell * 3
            }) {
                continue;
            }
            self.mc2_manifestation_tick(hx, spell, m, p, ctx);
        }
    }

    /// What a HUMAN manifestation's tick is about to do to the hydra
    /// `v34` dwords, read at dispatch entry: `Idle` for the call-free
    /// `word_0x2E_46 <= 0` exit, `MeteorFire(word)` for spell 9's fire
    /// tick (`word` = `sub_68E50`'s muzzle buffer, low dword). See
    /// [`crate::engine::features::no_mc2_m27_v34_token_transparent`] and
    /// [`crate::engine::features::no_mc2_m27_v34_meteor_cast_residue`].
    pub(crate) fn mc2_v34_token_pre(
        &self,
        hx: Hx,
        spell: usize,
        m: usize,
        p: PlayerPose,
    ) -> TokenV34 {
        let e = &self.g.ent[m];
        if e.f26 <= 0 {
            return TokenV34::Idle;
        }
        if spell == 9 && e.f26 as u16 == e.f28.max(1) && self.mc2_afford(hx, m) {
            let (mx, my, _) = self.muzzle_side(p, self.mc2_fire_side(hx, m));
            return TokenV34::MeteorFire(u32::from(mx) | u32::from(my) << 16);
        }
        TokenV34::Other
    }

    pub(crate) fn mc2_v34_token_post(&mut self, spell: usize, m: usize, v34: TokenV34) {
        match v34 {
            // Lightning's body `sub_6A5C0` zeroes its `[ebp-0x20]` local
            // — W-52 — BEFORE its idle gate.
            TokenV34::Idle if spell == 7 && !no_mc2_m27_v34_lightning_token_residue() => {
                let slot = &mut self.g.m27_v34_slot;
                slot.0 = Some(0);
                slot.1 = false;
                if let Some(p) = slot.3 {
                    if self.g.m27_v34_broken(p as usize, m) {
                        self.g.m27_v34_slot.2 = None;
                    }
                    self.g.m27_v34_slot.3 = Some(m as u16);
                }
            }
            // The castle's idle tail runs `sub_6D880`, which goes deep
            // only with a tier pending (`word_0x2C_44`).
            TokenV34::Idle
                if !no_mc2_m27_v34_token_transparent()
                    && spell != 7
                    && (spell != 2 || self.g.ent[m].f44 == 0) =>
            {
                self.g.m27_v34_transparent(m);
            }
            TokenV34::MeteorFire(word) if !no_mc2_m27_v34_meteor_cast_residue() => {
                // A window that closed on this tick with a pending tier
                // (`word_0x2C_44`) ran `sub_6D880`'s deep
                // `call 0x91de0` too: not modelled.
                let w64 = if self.g.ent[m].f26 == 0 && self.g.ent[m].f44 != 0 {
                    None
                } else {
                    Some(word)
                };
                let slot = &mut self.g.m27_v34_slot;
                slot.0 = None;
                slot.1 = false;
                slot.2 = w64;
                slot.3 = Some(m as u16);
            }
            _ => {}
        }
    }

    /// ONE manifestation's effect state — the body of
    /// [`World::mc2_cast_tick`]'s loop, split out because retail runs it
    /// as the class-15 entity's OWN action at its OWN pool slot, not
    /// from the caster's dispatch (see
    /// [`World::mc2_manifestation_pass`]).
    pub(crate) fn mc2_manifestation_tick(
        &mut self,
        hx: Hx,
        spell: usize,
        m: usize,
        p: PlayerPose,
        ctx: &MobCtx,
    ) {
        {
            // CASTLE (2) is not a timed cast — its "active" window is an
            // UPGRADE LOCK driven by the castle's transform, exactly like
            // retail's `sub_69AB0`/`sub_5F890` (the manifestation timer is
            // never counted down; the castle build/upgrade/DOWNGRADE
            // entity pins it and clears it on completion). It must be
            // evaluated every tick — including an externally-forced
            // downgrade the player never cast — so it lives outside the
            // `f26 > 0` gate below.
            if spell == 2 {
                self.mc2_castle_spell_tick(hx, m, p, ctx);
                if self.g.ent[m].f54 > 0 {
                    self.g.ent[m].f54 -= 1;
                }
                return;
            }
            // HEAL (5) has its OWN body — `sub_6A300` (EF:56430) — and
            // shares nothing with the skeleton below but the trailing
            // `word_0x36_54` decrement.
            if spell == 5 {
                self.mc2_heal_token_tick(hx, m);
                return;
            }
            if self.g.ent[m].f26 > 0 {
                // `sub_68DE0` (EF:55569) has two halves keyed on the
                // FIRST burst tick (`word_0x2E_46 == word_0x30_48`):
                // first tick stamps the negative-cost debit; EVERY
                // later live tick pins the caster's regen accumulator
                // to 0 — the "an active spell blocks mana
                // regeneration" law (docs/spell-audit/mana-regen.md).
                // Read `first` before the countdown.
                let first = self.g.ent[m].f26 as u16 == self.g.ent[m].f28.max(1);
                let afford = self.mc2_afford(hx, m);
                // ⭐ SPEED's `word_0xe_14` COLLAPSE IS THE SAME ARM AS
                // THE UNAFFORDABLE ONE. Retail's shape is
                // `if (!afford || v14) { if (v14) counter = 1 }` with
                // the ENTIRE effect body — boost write, puff, and
                // `sub_68DE0` — in the `else` (EF:56216-19). So a brake
                // tick skips the regen suppression exactly the way a
                // broke tick does, which is what the entry below
                // already says about the afford arm: the wizard's fresh
                // recompute stands on the very tick the window dies
                // (galore t=6279, `player.mana` off by one +1153 regen
                // step when the collapse suppressed it anyway).
                let v14_collapse = spell == 3 && self.mc1_v14[hx];
                // ⭐⭐ THE DUEL FIZZLE IS `sub_6B610`'s OWN `LABEL_19`,
                // AND LABEL_19 SKIPS THE WHOLE ELSE — including
                // `sub_68DE0` (EF:57277-57285). Retail's predicate is
                // exactly `word_0x2E_46 <= word_0x30_48 - 28 &&
                // !wizext->word_0x146_326`, with NO lower guard: the
                // port's extra `f26 > 1` blocked the arm in precisely
                // the case that matters, because the RETRIGGER FAMILY
                // pins `f26` to 1 first (`sub_5F660` case 0xE,
                // EF:60914-27 — duel is one of the six spells whose
                // re-press CANCELS instead of refusing).
                //
                // mc2l6-rsg t=1347 is both halves at once: the player
                // re-presses cast 21 ticks into a 195-tick duel window,
                // `sub_5F660` pins the counter to 1, the token's own
                // tick then reads `1 <= 167` with no lock and takes
                // LABEL_19 — so the mana-regen clamp never runs and the
                // wizard's freshly recomputed `d88` 713 stands. Retail
                // steps mana 1416916 → 1417629 at t=1348; the port
                // suppressed on the collapse tick and held. Same law as
                // the afford arm and the speed brake right above.
                let duel_fizzle = spell == 14
                    && self.mc2_duel[hx].is_none()
                    && (self.g.ent[m].f26 as i32) <= self.g.ent[m].f28.max(1) as i32 - 28;
                // ⭐⭐⭐ THE SHIELD-III STATEMENT ORDER — see
                // [`no_mc2_shield3_predecrement`]. `sub_6A480`'s
                // `life_0x1A == 1` arm (EF:56525-28) decrements `word_0x2E_46`
                // BEFORE calling `sub_68DE0`, so the counter that
                // function compares against `word_0x30_48` is already
                // one lower: the full-cost debit is unreachable and
                // the else-arm's regen pin is keyed on `f26 - 1`.
                let shield3_predecrement = spell == 6
                    && !no_mc2_shield3_predecrement()
                    && self
                        .g
                        .assets
                        .spells
                        .get(spell)
                        .map_or(Mc2SubSpell::default(), |r| {
                            r.tiers[(self.g.ent[m].f71 as usize).min(2)]
                        })
                        .life
                        == 1;
                if v14_collapse || duel_fizzle {
                    self.g.ent[m].f26 = 1;
                } else if afford {
                    if first {
                        self.mc2_spell_fire(hx, spell, m, p, ctx);
                        if shield3_predecrement {
                            // `sub_68DE0` with `v2 = f26 - 1`: the
                            // `v2 == word_0x30_48` debit is
                            // unreachable, so this takes the else —
                            // the mid-burst pin — on the ARM tick.
                            if self.g.ent[m].f26 > 1 {
                                self.suppress_regen(hx);
                            }
                        } else {
                            let cost = self.g.ent[m].max_life;
                            self.mana_debit(hx, cost);
                        }
                    } else if self.g.ent[m].f56 != 0 {
                        // The possess re-press RELEASE SIGNAL
                        // (`byte_0x3C_60`, raised by the cast gate's
                        // model-1 armed arm). Its effect-state consumer
                        // (`sub_68DE0` EF:55987-56013) is TIER-GATED on
                        // `byte_0x46_70`: for TIER 0 (plain possession)
                        // the signal is simply CLEARED — NO second bolt,
                        // NO mana debit — while the marker runs; only
                        // the higher tiers (Mana Magnet/Lock) re-spawn
                        // (`sub_69900`, on a 3-tick counter). Recorded
                        // retail (mc2l30/l0/l4, all tier-0 possess)
                        // fires exactly ONE delivery bolt per arm and
                        // none while armed — the earlier "re-cast
                        // freely, all tiers" reading over-fired (the
                        // (9,17) re-press family, no retail counterpart
                        // at any input latency: mc2l30 452->355, mc2l0
                        // 445->312, mc2l4 1393->1208).
                        //
                        // ⭐⭐ AND THE TIER'S RE-FIRE IS THE **BASIC**
                        // BOLT ON A 1..3 COUNTER, NOT THE TIER'S OWN
                        // ARM AND NOT A SECOND CAST. Retail's block is
                        // four statements (EF:55995-56010):
                        //   `v6 = byte_0x3C_60; if (v6) {`
                        //   `  if (byte_0x46_70) {`
                        //   `    if (v6 == 1) sub_69900(a1x, v1x);`
                        //   `    if (++byte_0x3C_60 > 3) { = 0; … } }`
                        //   `  else byte_0x3C_60 = 0; }`
                        // — `sub_69900` is the (9,1)/(10,12) BASIC
                        // spawner, so a held Mana Magnet or Mana Lock
                        // sprays PLAIN claim bolts between its own
                        // deliveries; the signal is a COUNTER that
                        // latches for three ticks, so a mashed button
                        // cannot re-fire faster than that; and there
                        // is NO debit — `sub_68DE0` parks the whole
                        // cost in the regen lane on the window's FIRST
                        // tick only (`v2 == word_0x30_48`, EF:55575-83)
                        // and every later tick merely pins regen to 0.
                        // mc2l6-rsg t=3993 slot 846 is the witness: the
                        // player holds tier 2 (token 345 `byte_0x46_70`
                        // 2, window tick 41 of 51) and retail borns a
                        // **(9,1) action 1** with `dword_0x10_16` 200
                        // and `byte_0x44_68` 12, where the port re-ran
                        // the tier walk and launched (9,17) action 18.
                        if self.g.ent[m].f71 != 0 {
                            if self.g.ent[m].f56 == 1 {
                                // `sub_68E50` reads the TOKEN, so the
                                // held tier's own row still supplies
                                // the payload columns.
                                let tier = (self.g.ent[m].f71 as usize).min(2);
                                let sub = self
                                    .g
                                    .assets
                                    .spells
                                    .get(spell)
                                    .map_or(Mc2SubSpell::default(), |r| r.tiers[tier]);
                                self.mc2_possess_launch(hx, m, p, 1, (10, 12), sub);
                            }
                            let v7 = self.g.ent[m].f56.wrapping_add(1);
                            self.g.ent[m].f56 = if v7 > 3 { 0 } else { v7 };
                        } else {
                            self.g.ent[m].f56 = 0;
                        }
                    }
                } else if spell != 3 || no_mc2_speed_broke_survives() {
                    // Can't afford mid-cast → collapse to one tick
                    // (EF:63... skeleton line `word_0x2E_46 = 1`).
                    // ⭐⭐⭐ …EXCEPT SPEED, whose unaffordable arm
                    // collapses ONLY on the brake word — see
                    // [`no_mc2_speed_broke_survives`].
                    self.g.ent[m].f26 = 1;
                }
                // Mid-burst regen suppression (`sub_68DE0` else
                // branch): the first-tick debit already drove
                // `mana_delta` negative, so the `> 0` guard preserves
                // it; every later tick clamps the positive regen the
                // wizard tick recomputed this frame (world.rs:1225).
                // AFFORD-GATED: every retail handler calls sub_68DE0
                // only on the afford-success path — the collapse arm
                // (`word_0x2E_46 = 1`, EF:55885/56021/56349/56468/
                // 56690) skips it, so the wizard's fresh recompute
                // stands on the very tick the window dies (mc2l3
                // t=1496: purse 200 < fireball 250, retail regen +100
                // lands immediately, the port held it one tick).
                //
                // BELOW-CARPET ORDERING: retail's clamp at frame F
                // hits the delta recomputed at F-1 — the value the
                // wizard body (above the token) is about to apply
                // THIS frame — so the clamped applies run exactly
                // while the token still SEES a live window, and regen
                // pays on the token's first INERT frame. That falls
                // out on its own now that the wizard body's mana
                // block runs at the carpet's walk slot instead of
                // pre-walk; the stash-and-pay-back window this used
                // to need (mc2l3 t=9037-9055, the possession spam —
                // token slot 109 < carpet 167, one +100 row per cast
                // cycle) is gone with it. ⚠ A skip on the f26==1 tick
                // is still NOT equivalent: the retrigger family
                // (shield/invis re-presses, EF:60914-28) pins f26 at
                // 1 for the whole hold and retail clamps every one of
                // those ticks.
                // ⭐⭐⭐ WHICH SPELLS PIN THE REGEN EVERY TICK IS AN
                // ENUMERATED LIST, AND SIX OF THEM DO NOT.
                // `sub_68DE0` is not part of a shared skeleton — each
                // of the 26 manifestation handlers places its own call,
                // and the placement is not uniform. Twenty put it
                // AFTER the `word_0x2E_46 == word_0x30_48` block, so
                // it runs on every afforded tick (the first-tick arm
                // parks `-maxMana` in the regen lane, later ticks clamp
                // the positive regen to 0 — the "an active spell blocks
                // mana regeneration" law). But `sub_6BCF0` (17),
                // `sub_6BF30` (18), `sub_6C620` (21), `sub_6C870` (22),
                // `sub_6CAC0` (23) and `sub_6CFA0` (25) close the block
                // one brace LATER, with the call INSIDE it — so those
                // six debit on the arm frame and then leave the purse
                // alone for the rest of their window. (Castle 2 and
                // heal 5 are the same shape and already return above;
                // 1 and 7 nest an extra call but also carry an
                // unconditional trailing one, so they pin every tick.)
                // The port suppressed generically for all of them.
                //
                // mc2l6-rsg t=6267: the human casts spell 18 for 12000
                // (`d88` 719 -> -12000, purse 1438596 -> 1426596) on a
                // 23-tick window, and retail pays the full +719 regen
                // on t=6268->6269 and EVERY tick after while token 362
                // counts down 21, 20, 19… — 1427315, 1428034, 1428753.
                // The port held the purse flat for the whole window.
                if !first
                    && afford
                    && !v14_collapse
                    && !duel_fizzle
                    && !NO_MID_BURST_REGEN_PIN.contains(&spell)
                    // ⭐⭐⭐ …AND SHIELD III'S PIN IS KEYED ON THE
                    // POST-DECREMENT COUNTER, so the tick the window
                    // ENDS on (`f26 == 1`, which the re-press cancel
                    // `sub_5F660` case 0xE also manufactures) pins
                    // NOTHING and the wizard's fresh recompute stands.
                    && !(shield3_predecrement && self.g.ent[m].f26 <= 1)
                {
                    self.suppress_regen(hx);
                }
                // ⭐⭐⭐ AND SO IS THE SHIELD WINDOW — THE LAST OF THE
                // FOUR WIZARD-BUFF COLUMNS TO GET IT.
                // `sub_6A480` (EF:56496-56541) is the shield's exact
                // twin of the rebound body below, on the OWNER's
                // (`parentId_0x28_40`) flag word, keyed on the TIER's
                // `life_0x1A` and not on the tier index:
                //
                // ```text
                //   v1 = SPELLS[6].subspell[byte_0x46_70].life_0x1A;
                //   if (!v1)      { if (sub_68D50) { parent.byte[1] |= 0x40;   // CHARGED, EVERY tick
                //                                    sub_68DE0; word_0x2E_46--; } else word_0x2E_46 = 0; }
                //   else if (v1 == 1) { if (sub_68D50) { if (word_0x2E_46 == word_0x30_48)
                //                                            parent.byte[2] |= 0x40;  // ARMED, FIRST tick only
                //                                        word_0x2E_46--; sub_68DE0; } else word_0x2E_46 = 0; }
                //   if (!word_0x2E_46) { parent.dword &= 0xFFBFBFFF; sub_6D880; }
                // ```
                //
                // SPELLS.DAT gives spell 6 `life_0x1A` = 0 / 0 / 1
                // (durations 101 / 201 / 301), so Shield I and II are
                // the CHARGED stamp — re-published on EVERY afforded
                // tick — and only Shield III is the one-shot ARMED
                // stage.
                //
                // The port latched `player.shield` ONCE, in
                // `mc2_spell_fire`'s spell-6 arm, and
                // `apply_player_damage` CLEARS it per absorb
                // (world.rs, the `:55700-07` block). So a human
                // Shield I quartered exactly ONE hit and then ran
                // naked for the remaining 100 ticks of its own
                // window, where retail quarters — and mana-pays —
                // every hit for the whole window. THREE of the four
                // wizard-buff columns already held this law: MC1's
                // human (`world.rs` spell 4, "SET-only … the damage
                // intake's absorb is the only clear"), MC1's rival
                // (`mc1/rivals.rs`, `flags & 0x4000`) and — since the
                // MC2 rival buff-bits dig — `mc2/rivals.rs`
                // ([`crate::mc2::rivals::F_SHIELD_CHARGED`]). Only the
                // MC2 human never crossed.
                //
                // ⚠ OPEN, and reported as an anchored hunk rather than
                // landed here: the ARMED stage (`byte[2] & 0x40`) has
                // no lane on the human column at all — `Player` has
                // only the one `shield` bool — so Shield III keeps
                // `mc2_spell_fire`'s FIRST-TICK-ONLY stamp as its
                // stand-in, which is exactly what every tier had
                // before this block. That first-tick latch is left
                // where it is, so this block is purely additive and
                // `MGC_NO_SHIELD_RESTAMP=1` is an exact revert.
                if spell == 6 && afford && !no_shield_restamp() {
                    let tier = self.g.ent[m].f71 as usize;
                    let life = self
                        .g
                        .assets
                        .spells
                        .get(spell)
                        .map_or(Mc2SubSpell::default(), |r| r.tiers[tier.min(2)])
                        .life;
                    if life == 0 {
                        self.set_human_shield(hx, true);
                    }
                    // ⭐⭐⭐ AND THE OTHER ARM OF THE SAME `if` IS THE
                    // ARMED STAGE — `else if (v1 == 1) { if (sub_68D50)
                    // { if (word_0x2E_46 == word_0x30_48)
                    // parent.byte[2] |= 0x40; … } }` (EF:56525-31).
                    // FIRST TICK ONLY, and a tier declaring `life_0x1A`
                    // > 1 stamps NEITHER bit: retail's chain is
                    // `if (!v1) … else if (v1 == 1) …` with no trailing
                    // else, an ENUMERATED LIST whose absence is a law.
                    // mc2l6-rsg has three of these windows on the human
                    // (t=22531 / 22730 / 23116, `sel[6] == 2` and
                    // `SPELLS[6].tiers[2].life == 1`).
                    if life == 1 && first && !no_mc2_shield_armed() {
                        self.set_human_shield_armed(hx, true);
                    }
                    if std::env::var_os("MGC_SHIELD_PROBE").is_some() {
                        let t = crate::DEBUG_TICK.load(std::sync::atomic::Ordering::Relaxed);
                        eprintln!(
                            "[shield] t={t} tier={tier} life={life} f26={} f28={} first={first}",
                            self.g.ent[m].f26, self.g.ent[m].f28
                        );
                    }
                }
                // ⭐⭐⭐ THE REBOUND WINDOW IS A FLAG THE TOKEN
                // RE-STAMPS AT ITS OWN WALK SLOT, EVERY AFFORDED
                // TICK — NOT A BOOLEAN THE CAST LATCHES.
                // `sub_6AA00` (EF:56721-51) is three statements on the
                // OWNER's flag word: window dead → `word[0] &= 0x7FEF`
                // (both bits), else the tier's own bit, `life_0x1A == 0`
                // → `byte[1] |= 0x80` (scatter) and `== 1` →
                // `byte[0] |= 0x10` (precise), and NEITHER for a tier
                // that declares anything else. The deflect gate reads
                // it live off the victim's record at the projectile's
                // walk slot (`v4x->…word[0] & 0x8010`, EF:62939), so
                // the ARM tick already deflects for every bolt below
                // the token — the book sits at 344+ and the human's
                // cast runs at the carpet's own slot 343, above it.
                // The port derived `Gen::player_rebound` once at the
                // tick head (world.rs), so the whole arm frame read
                // stale: mc2l6-rsg t=4847 is the cast, retail's token
                // 352 loads its 251-tick window and stamps 0x8000 on
                // the carpet the same frame, and rival 370's (9,0) at
                // slot 809 — already inside the human — comes back
                // owned by the human with `roll` 15 / `yaw` 57 and
                // life refilled, where the port let it land and reaped
                // it. ⭐ THE MC1 COLUMN HAS HELD THIS LAW SINCE
                // mc1hwl0 t=38740 (`world.rs` spell 14's mid-walk
                // republish); it simply never crossed.
                if spell == 8 && afford {
                    let tier = self.g.ent[m].f71 as usize;
                    let life = self
                        .g
                        .assets
                        .spells
                        .get(spell)
                        .map_or(Mc2SubSpell::default(), |r| r.tiers[tier.min(2)])
                        .life;
                    let precise = match life {
                        0 => Some(false),
                        1 => Some(true),
                        _ => None,
                    };
                    if let Some(precise) = precise {
                        self.players[hx].rebound = true;
                        self.g.set_player_rebound(hx, true);
                        self.g.mc2_rebound_precise[hx].0 = precise as i32;
                    }
                }
                // ⭐⭐ SPEED'S OWN REGISTER WRITE (`GetScroll_69DB0`
                // EF:56230-40), the thing that makes it a spell and not
                // a thrust model:
                //
                //     v2 = sign(carpet->speed_0xc_12)
                //     if (counter == max)  speed = v2*minSpeed*(sub+1)
                //     else                 speed = v2*minSpeed* sub
                //     carpet->actSpeed = speed
                //
                // ⭐ THE FIRST TICK OF EVERY WINDOW IS ONE FACTOR
                // HOTTER. `subSpellIndex_2` is {2,3,4} by tier, so the
                // sustained speeds are 160/240/320 and each arm — or
                // RE-arm, since a re-press reloads the counter to its
                // max — spikes 240/320/400 for exactly one tick. That
                // is not cosmetic: the spike is the value the NEXT
                // tick's `sub_5D530` polar-steps on, so it moves the
                // carpet a whole 240-unit stride. galore records the
                // full matrix (sustained 160×371 / 240×295 / 320×200
                // and backward −240/−320/−400), and every one of its
                // ~300 spike ticks is a `move_bits & 0x10` re-press.
                //
                // The port had the sustained magnitude right (the
                // baked `sub_spell`, cached on the token as `f30`) but
                // synthesised it inside the mover from a `speed_boost`
                // recomputed at the PREVIOUS tick's tail, so it had
                // neither the spike nor the walk-order phase.
                // docs/spell-audit/speed.md §5 had already written the
                // spike down and filed it as an "optional fidelity
                // nicety".
                //
                // ⚠⚠ The direction is retail's `sign(speed_0xc_12)` —
                // the COMMAND, and this comment used to end *"the
                // carpet is out of pool here, so the pose's `speed`
                // (`actSpeed`) stands in; … on the arm tick they
                // differ only while a decelerating carpet crosses
                // zero."* That dismissal WAS the defect: mc2l33
                // t=8896 is exactly a decelerating carpet crossing
                // zero, and it cost the take its certification. The
                // command now reaches this seat as
                // `World::mc2_cmd_speed` — see
                // [`no_mc2_speed_sign_cmd`].
                if spell == 3 && afford && !v14_collapse {
                    let dir_src = if no_mc2_speed_sign_cmd() {
                        p.speed
                    } else {
                        self.mc2_cmd_speed[hx]
                    };
                    let sign = if dir_src >= 0 { 1i16 } else { -1 };
                    let factor = self.g.ent[m].f30 as i16 + i16::from(first);
                    self.pending_speed_base[hx] = Some(sign * 80 * factor);
                }
                // SPEED's slipstream trail (`GetScroll_69DB0`
                // EF:56251-59): every 4th tick of the live window
                // — keyed on the TOKEN's phase byte
                // (`byte_0x3E_62 & 3`, our f63, NOT the caster's) —
                // drop a (10,2) ambient puff at the CASTER with the
                // ctor's life QUADRUPLED (8 → 32) and the caster's id.
                // `NewAdd0A02_4E430` (EF:35375) is a bare 4-field
                // ctor: maxLife 8, action 2, no sprite, and NO map
                // link (it writes `position_0x4C_76` directly), which
                // is why the trail hangs in the air where the carpet
                // was. mc2l3 t=15500+ is the instrument: one puff
                // every 4 ticks marching along the boosted flight
                // path, 175 of them across the take.
                // ⭐ AND THE PUFF LIVES INSIDE THE SAME `else` AS THE
                // REGISTER WRITE (EF:56602-10) — an unaffordable tick
                // spawns nothing. See [`no_mc2_speed_broke_survives`]:
                // this gate is inert while the window still collapsed
                // on the broke tick, because the collapse killed the
                // token before a fourth tick could come round.
                if spell == 3
                    && (afford || no_mc2_speed_broke_survives())
                    && !v14_collapse
                    && self.g.ent[m].f63 & 3 == 0
                    && let Some(s) = self.g.mc2_spawn_speed_puff(p.x, p.y, p.z)
                {
                    self.g.ent[s].act_life *= 4;
                    self.g.ent[s].id24 = self.g.human_tag[hx].0;
                }
                self.g.ent[m].f26 -= 1;
                if self.g.ent[m].f26 == 0 {
                    // The window's LAST act is to hand the carpet back
                    // its 1× base, sign kept (EF:56266-68) — retail's
                    // restore is this write, at the token's own slot,
                    // not an expiry edge the carpet notices a tick
                    // later. MC1's `pending_speed_base` mail is the
                    // same seam and needed no new machinery.
                    if spell == 3 {
                        let dir_src = if no_mc2_speed_sign_cmd() {
                            p.speed
                        } else {
                            self.mc2_cmd_speed[hx]
                        };
                        self.pending_speed_base[hx] = Some(if dir_src >= 0 { 80 } else { -80 });
                    }
                    self.mc2_cast_expire(hx, spell, m);
                }
            }
            if self.g.ent[m].f54 > 0 {
                self.g.ent[m].f54 -= 1;
            }
        }
    }

    /// **MC2 HEAL — `sub_6A300` (EF:56430-56477).** The human column's
    /// heal had NO effect tick at all: `mc2_spell_fire` set
    /// `player.heal_active`, the expiry cleared it, and nothing in the
    /// crate ever read it (the only `player.life` writers were the
    /// cheats, the respawn, the passive regen and MC1's two heal
    /// sites). Casting heal in MC2 restored nothing.
    ///
    /// The body is MC1's `sub_56270` skeleton with ONE structural
    /// difference, and that difference is the whole player-facing
    /// character of the spell:
    ///
    /// - **MC1** puts `actLife < maxLife` INSIDE the admission
    ///   (`sub_55DD0 && v1[3] < v1[2] && v1[35] >= +136`,
    ///   remc1 :65101-03), so a full-life tick takes the `else` and
    ///   RELEASES (`word_0x30_48 = 1`). The release is terminal —
    ///   nothing in retail resurrects a zeroed counter.
    /// - **MC2** admits on the gate and the purse ALONE and nests the
    ///   life test inside the accepted branch (:56445/:56450). A
    ///   full-life tick therefore falls through doing *nothing*: no
    ///   heal, no XP, **no debit** — and the window STAYS OPEN and
    ///   keeps re-testing. Get hurt while it runs and it starts
    ///   healing and charging again, mid-window.
    ///
    /// So the player's report — *"heal costs nothing if there's
    /// nothing to heal, no matter how many times it is cast, even
    /// though it appears active… if hurt while heal is active, it will
    /// start costing and healing again"* — is a composite of the two
    /// games: the first half holds in both, the second is MC2-only and
    /// structurally impossible in MC1 (see the SESSION 73 entry, where
    /// the MC1 half was measured and the mechanism read backwards from
    /// an identical observable).
    ///
    /// The other MC2-specific details, all from the same lines:
    /// - the amount is `maxLife * subSpellIndex_0x2A_42 / 100` (the
    ///   TIER drives the percentage) where MC1's is a flat 5%;
    /// - the admission carries an EXTRA `mana >= maxMana_0x8C_140` leg
    ///   on TOP of `sub_68D50` (:56445), so unlike every other MC2
    ///   spell heal re-checks affordability EVERY tick, not just the
    ///   `word_0x2E_46 == word_0x30_48` one — run dry mid-window and
    ///   it collapses;
    /// - the debit is the FULL cost stamped on the regen delta on
    ///   every HEALING tick (:56458-61), not a one-shot at the arm;
    /// - `sub_6A300` never calls `sub_68DE0`, so heal does NOT pin
    ///   mid-burst regen the way the skeleton's spells do;
    /// - the XP award (`sub_6D8B0(parent, 5, 1)` :56452) is on the
    ///   first HEALING tick, so a cast at full life scores nothing;
    /// - sound 25 is on the first ADMITTED tick, healing or not.
    fn mc2_heal_token_tick(&mut self, hx: Hx, m: usize) {
        if self.g.ent[m].f26 > 0 {
            let first = self.g.ent[m].f26 as u16 == self.g.ent[m].f28.max(1);
            let cost = self.g.ent[m].max_life;
            // `sub_68D50` is `mc2_afford`; the second leg is heal's own
            // per-tick purse test (:56445).
            let admitted =
                self.mc2_afford(hx, m) && (self.dev_spells || self.human_mana(hx) >= cost);
            if admitted {
                if first {
                    self.g.snd_player(hx, 25);
                    self.players[hx].heal_active = true;
                }
                if self.human_life(hx) < PLAYER_LIFE_MAX {
                    if first {
                        self.mc2_award_xp(self.g.human_tag[hx].0, 5, 1);
                    }
                    // Retail's operand order: `maxLife * sub / 100`.
                    let step = (PLAYER_LIFE_MAX as i64 * self.g.ent[m].f30 as i64 / 100) as i32;
                    self.set_human_life(hx, (self.human_life(hx) + step).min(PLAYER_LIFE_MAX));
                    self.mana_debit(hx, cost);
                }
            } else {
                // :56466 — the refusal RELEASES; the decrement below
                // expires it next tick.
                self.g.ent[m].f26 = 1;
            }
            self.g.ent[m].f26 -= 1;
            if self.g.ent[m].f26 == 0 {
                self.mc2_cast_expire(hx, 5, m);
            }
        }
        if self.g.ent[m].f54 > 0 {
            self.g.ent[m].f54 -= 1;
        }
    }

    /// The CASTLE spell (2) tick — the UPGRADE LOCK, ported from retail's
    /// `sub_69AB0` + `sub_5F890` (EF:56086/61029). The manifestation's
    /// cast timer `f26` (`word_0x2E_46`) is NEVER a countdown for the
    /// castle: on the cast tick it fires once (spawns the build ball) and
    /// commits the cost, then it is HELD at `f28 - 1` (`word_0x30_48 - 1`)
    /// while the castle is transforming and cleared to 0 the moment the
    /// transform completes — so the "cast in progress" glow, and the
    /// re-cast block in [`World::mc2_cast_gate`], last exactly as long as
    /// the tower build/upgrade/downgrade, not a fixed 101 ticks. Because
    /// the lock is driven by the transform (not a cast), an externally
    /// forced downgrade (an enemy razing the castle level by level) also
    /// raises it — you get the "split second between transforms" to cast
    /// a rebuild, faithful to both games.
    fn mc2_castle_spell_tick(&mut self, hx: Hx, m: usize, p: PlayerPose, ctx: &MobCtx) {
        // ⭐⭐⭐ `sub_69AB0` OPENS WITH THE SPENT-TIMER ARM, AND IT IS
        // THE ONLY PLACE THE MANIFESTATION'S DEFERRED TIER IS EVER
        // APPLIED (EF:56436-39):
        // ```text
        //   if (a1x->word_0x2E_46 <= 0) { sub_6D880(a1x); }
        //   else { …the whole body… }
        // ```
        // The release that ZEROES the timer does NOT apply it: retail's
        // `sub_5F890` `a2 == 0` arm (EF:61381-84) zeroes the
        // MANIFESTATION's `word_0x2E_46` and then calls `sub_6D880` on
        // **`a1x`, the CASTLE** — the record `sub_5F890` was invoked on,
        // not the manifestation it just released. The shipped EXE is
        // unambiguous (`NETHERW.EXE` file **0x840BC-0x840CB**, linear
        // 0x5F8BC; MC2 file = VA + 0x24800), with `edx` still holding
        // the `a1x` argument loaded at 0x84094:
        // ```text
        //   840bc: 0f bf c3          movswl %bx,%eax        ; SpellEnabled[2]
        //   840bf: 8b 04 85 ...      mov 0x1a3e4(,%eax,4),%eax ; the MANIFESTATION
        //   840c6: 52                push  %edx             ; ← the CASTLE
        //   840c7: 66 89 48 2e       mov   %cx,0x2e(%eax)   ; manifestation->w2E = 0
        //   840cb: e8 b0 df 00 00    call  0x92080          ; sub_6D880
        // ```
        // So the pending tier survives the release and lands on the
        // manifestation's NEXT tick, through this entry arm — a clean
        // ONE-TICK LAG.
        //
        // WITNESS — mc2l7, slot 38, the human's (15,2) castle
        // manifestation, with `word_0x2C_44 = 2` pending (tier 1):
        //   t=25588 retail: castle slot 59 returns to the standing idle
        //           (`action` 5 → 4) and the release zeroes slot 38's
        //           `word_0x2E_46` 100 → 0 — and NOTHING else moves.
        //   t=25589 retail: `word_0x2C_44` 2 → 0, `byte_0x46_70` 0 → 1,
        //           `maxMana_0x8C` 10000 → 12500, `mana_0x90` 99 → 123.
        // The port did the whole stamp at 25588 (the release's own
        // `mc2_cast_expire`) and then, importing retail's 25588, had no
        // arm left to run at 25589 — the two-tick reset cluster
        // 25588/25589 in the take's census, the second row the exact
        // inverse of the first.
        //
        // `MGC_NO_MC2_CASTLE_TIER_DEFER_LAG=1` restores the old shape.
        //
        // The `return` is retail's `else` (EF:56436-39): a spent word
        // runs ONLY `sub_6D880`. It is unconditional under the latch
        // (round 135). ⚠ In the re-derive arm
        // (`MGC_NO_MC2_CASTLE_CAST_LATCH`) it must fall through: that
        // arm's live scan in the tail is the only thing able to RAISE
        // the lock from 0, and a bare return there decertified
        // mc2l4-new (END -> 552) and mc2l1-new (END -> 3200) with
        // duplicate (9,10) balls (round 134-3).
        if !no_mc2_castle_tier_defer_lag() && self.g.ent[m].f26 <= 0 {
            self.mc2_cast_expire(hx, 2, m);
            if !no_mc2_castle_cast_latch() {
                return;
            }
            // Fall through to the re-derive tail; the cast arm below
            // cannot fire from `f26 <= 0` (`dur >= 1`).
        }
        let dur = self.g.ent[m].f28.max(1) as i16;
        if !no_mc2_castle_cast_latch() {
            // ⭐⭐⭐ RETAIL'S `sub_69AB0` BODY, IN ITS OWN ORDER
            // (EF:56440-56515): `if (sub_68D50(a1x, caster))` wraps
            // the fresh-cast arm and its ELSE is the ONLY release in
            // the function — VA 0x69D42 `movw $0x0,0x2e(%eax)`. So a
            // held word (dur - 1) is released here only when the
            // caster can no longer carry it (dead / upkeep unmet), and
            // a fresh word (== dur) when the full cost is not there.
            if self.g.ent[m].f26 > 0 && !self.mc2_afford(hx, m) {
                self.g.ent[m].f26 = 0;
                return;
            }
            // A fresh cast arms `f26 = f28` in `mc2_cast_gate`; that
            // sentinel is the only entry that fires + debits (the
            // transform pins 100 = dur - 1, never dur).
            if self.g.ent[m].f26 == dur {
                self.mc2_spell_fire(hx, 2, m, p, ctx); // cast_castle: spawns the ball
                let cost = self.g.ent[m].max_life;
                self.mana_debit(hx, cost);
                // THE PIN — VA 0x69BAE, in the ball spawn's own
                // straight-line block and gated on the spawn having
                // returned a slot (`test %eax,%eax / je`). The port
                // has no spawn handle, so it reads the pool the ball
                // has just been placed in. A ball whose own first
                // dispatch (same tick, higher slot) refuses the site
                // releases it again through the ball-side mail —
                // mc2l1-new t=3604.
                if self.mc2_castle_ball_aloft(hx) {
                    self.g.ent[m].f26 = dur - 1; // word_0x30_48 - 1
                }
            }
            return;
        }
        // ---- the pre-135 RE-DERIVE arm (`MGC_NO_MC2_CASTLE_CAST_LATCH`) ----
        if self.g.ent[m].f26 == dur {
            if self.mc2_afford(hx, m) {
                self.mc2_spell_fire(hx, 2, m, p, ctx);
                let cost = self.g.ent[m].max_life;
                self.mana_debit(hx, cost);
            } else {
                self.g.ent[m].f26 = 0;
                return;
            }
        }
        // Re-derive `sub_5F890`'s state every tick from a live scan.
        // Retail does no such thing — the word is a LATCH, released
        // only by the castle-side and ball-side `sub_5F890` sites.
        let active = self.mc2_castle_lock_active(hx);
        let was = self.g.ent[m].f26 > 0;
        if active {
            self.g.ent[m].f26 = dur - 1; // word_0x30_48 - 1
        } else if was {
            self.mc2_castle_lock_release(hx, m);
        }
    }

    /// `sub_5F890`'s `a2 == 0` arm on the MANIFESTATION side: drop the
    /// upgrade lock and re-price the cached cast cost.
    ///
    /// `sub_60780` (EF:61670): every castle HP/CAP stamp also re-runs
    /// SetSpell on the manifestation's OWN tier (deferral suppressed —
    /// retail zeroes word_46 around the call), so the cached cast cost
    /// (`max_life`, the mana gate's word) tracks the castle level BOTH
    /// ways — including a DOWNGRADE, which awards no XP (demolish or an
    /// enemy razing a level would otherwise leave the old rung cached
    /// and ding an affordable rebuild as unaffordable). Ported at the
    /// lock-release edge instead of retail's mid-transform stamp — and
    /// since [`World::mc2_castle_lock_stamp`] moved that edge to the
    /// CASTLE's own pass, both writers now sit at the same slot retail
    /// runs them from. Retail's stamp rides the castle's own HP/CAP
    /// writes, so castle DEATH leaves the old rung cached (the MC2 face
    /// of the first-castle lockout): the castle-less release skips
    /// the re-sync exactly like retail. (The retired
    /// `castle_recast_cost` patch used to re-sync it to the base-cost
    /// rebuild here — gone 2026-09-07, player-ruled.)
    fn mc2_castle_lock_release(&mut self, hx: Hx, m: usize) {
        self.g.ent[m].f26 = 0;
        // ⚠ RETAIL'S `sub_6D880` HERE TAKES THE **CASTLE**, NOT `m` —
        // see the entry arm in [`Self::mc2_castle_spell_tick`] for the
        // EXE bytes. The manifestation's pending tier is applied one
        // tick later, by its own handler's spent-timer arm.
        if no_mc2_castle_tier_defer_lag() {
            self.mc2_cast_expire(hx, 2, m);
        }
        if self.seat_castle(hx).is_some() {
            let tier = self.g.ent[m].f71;
            self.mc2_set_spell(hx, m, tier);
        }
    }

    /// ⭐⭐⭐ `sub_5F890` (EF:61029) RUN AT THE **CASTLE'S** OWN PASS —
    /// the CREATE-CASTLE UPGRADE-LOCK, published one dispatch slot
    /// before the carpet instead of one after.
    ///
    /// Retail resolves the castle owner's spell-2 manifestation
    /// (`Entities[castle->id]->player->SpellsEnabled[2]`) and either
    /// PINS it at `word_0x30_48 - 1` (`a2 != 0`) or RELEASES it to 0
    /// and runs `sub_6D880` on the castle (`a2 == 0`). Every call site
    /// is the castle's own handler:
    ///
    /// * `EndOfCastleProjectile_5F8F0` action 4 — PIN on each
    ///   blast-shake countdown tick (EF:61076; the `== 1` release tick
    ///   transitions without pinning, so the census is on the PRE
    ///   `word_0x30_48` being >= 2),
    /// * `BeginOfCastleCreation_5FA70` action 5 — **RELEASE** on case 2
    ///   (EF:61151, the settle back to standing), PIN on cases 3 and 5
    ///   (EF:61155/61173),
    /// * `sub_605E0` from action 6 — PIN on each level actually taken
    ///   off (EF:61643) and RELEASE once the level reaches 0
    ///   (EF:61662, `sub_5F890(a1x, a1x->dword_0x10_16)`).
    ///
    /// **WHY THE SLOT IS THE WHOLE LAW.** The castle sits far BELOW the
    /// carpet in the ascending walk (mc2l6 castle 63, carpet 343,
    /// manifestation 346), and the human's cast gate runs at the
    /// carpet. The port published the release from
    /// [`World::mc2_castle_spell_tick`] — the manifestation's own pass,
    /// one slot ABOVE the carpet — so on the frame a build settles the
    /// gate still read the pinned 100, took `sub_5F660`'s `case 2:
    /// word_0x2E_46 > 0` buzz arm (EF:60908-13), and a HELD cast button
    /// lost that whole frame. mc2l6-rsg t=324: the (10,42) painter at
    /// slot 60 signals `f59 = 2`, castle 63 settles to action 4 and
    /// releases, and the human's held button re-casts THE SAME TICK —
    /// retail mints the (10,43) delivery pair and debits 30000 where
    /// the port fired nothing until t=328.
    ///
    /// ⭐ OWNER-GENERIC. Retail's `SpellsEnabled[2]` is materialized for
    /// every player including AI (`sub_5CF40` EF:59374), and EF:61036
    /// indexes it off `Entities_EA3E4[castle->id_0x1A_26]` with no
    /// player test at all — a RIVAL castle stamps its own owner's
    /// manifestation exactly like the human's does.
    ///
    /// ⚠ THE PORT'S OWN COMMENT HERE USED TO SAY THE LAW WAS INERT
    /// because "MC2 rivals cast through `mc2_rival_cast_castle`, which
    /// owns no class-15 record for the stamp to land on." That claim
    /// was false: the per-rival [`Mc2Spellbook`] carries `book.ent[2]`
    /// (`mc2_rival_castle_tier` reads its tier off exactly that
    /// record), and mc2l6-rsg slot 373 is rival 370's live (15,2).
    /// Retail pins it at t=4310 and the port left it at 0 for the
    /// 2,000 ticks that followed. (Fourth session running that a port
    /// comment named its own defect — cf. 81's "cannot be landed",
    /// 84's `.max()`, 85's "not in the current corpus".)
    pub(crate) fn mc2_castle_lock_stamp(&mut self, own: u16, pin: bool) {
        // ⭐⭐⭐ RETAIL'S ONLY GUARD IS `index != 0` (0x840AF-0x840B2).
        // A book marker of `1` therefore lands on POOL SLOT 1 whatever
        // class sits there, and the port's `@0x2E` home for that class
        // is `f46` (or `lease2e` on class 5), never `f26`. See
        // [`no_mc2_castle_lock_stray_2e`] for the whole disassembly and
        // the witness list.
        if let Some(m) = self.mc2_owner_castle_token_raw(own) {
            if self.g.ent[m].class64 != 15 {
                if !no_mc2_castle_lock_stray_2e() {
                    let v = if pin {
                        castle_lock_pin_value(self.mc2_raw_at_30(m))
                    } else {
                        0
                    };
                    self.mc2_write_raw_at_2e(m, v);
                }
                return;
            }
        }
        let Some(m) = self.mc2_owner_castle_token(own) else {
            return;
        };
        if pin {
            self.g.ent[m].f26 = self.g.ent[m].f28.max(1) as i16 - 1;
        } else if self.g.ent[m].f26 > 0 {
            if self.g.ent[m].model65 != 2 {
                // A MARKER-1 index resolved to another class-15
                // record (see `mc2_owner_castle_token`): retail's
                // `a2 == 0` arm writes that record's `word_0x2E_46 = 0`
                // and runs `sub_6D880` on the CASTLE — no manifestation
                // re-price exists for it.
                self.g.ent[m].f26 = 0;
            } else if let Some(hx) = self.hx_of_tag(own) {
                self.mc2_castle_lock_release(hx, m);
            } else {
                self.mc2_rival_castle_lock_release(m, own);
            }
        }
    }

    /// ⭐⭐⭐ `sub_605E0`'s LEVEL-0 **RIVAL** ARM (EF:61645-61658;
    /// NETHERW.EXE file 0x84f02-0x84f55) — THE CASTLE-DEATH TOKEN
    /// PURGE, and the ONE guard it has is `index != 0`.
    ///
    /// ```text
    ///   84f02  movsx esi,[ebx+0x1a]            ; castle->id_0x1A_26
    ///   84f06  mov   esi,[esi*4+0x1a3e4]       ; Entities[owner]
    ///   84f0d  cmp   byte [esi+0x40],1         ; owner->model == 1 (a RIVAL)
    ///   84f11  jne   0x84f57                   ; else → sub_5F890(castle, 0)
    ///   84f13  mov   eax,ds:0x41a0
    ///   84f18  test  byte [eax+0x2fed2],4      ; the LEVEL-GRAPHICS bit 2
    ///   84f1f  je    0x84f61
    ///   84f21  mov   eax,[esi+0xa4]            ; owner's player block
    ///   84f27  mov   dx,[eax+0x337]            ; SpellsEnabled[2] (0x333 + 2*2)
    ///   84f2e  test  dx,dx
    ///   84f31  je    0x84f61                   ; ← THE ONLY GUARD
    ///   84f33  movsx eax,dx
    ///   84f36  mov   edi,[eax*4+0x1a3e4]       ; Entities[idx] — NO class/model test
    ///   84f3d  push  edi
    ///   84f3e  call  0x7c710                   ; DisableEntityDrawing04_57F10
    ///   84f4c  mov   word [eax+0x337],0        ; SpellsEnabled[2] = 0
    /// ```
    ///
    /// ⭐ AND THE INDEX IS ROUTINELY A MARKER, NOT A SLOT. The wizard
    /// death handler `sub_5E310` (EF:60146,
    /// [`World::mc2_scatter_spells`]) stamps a **boolean 1** into every
    /// occupied book slot when a wizard dies. So a rival that died and
    /// then lost its castle hands this arm the literal `1` — and retail
    /// dereferences it, reap-flagging **whatever record sits at pool
    /// slot 1**. On mc2l22 that is the authored `(10,45)` building at
    /// slot 1: rival 477 is a corpse `(3,1)` in action 3, its castle
    /// 502 falls level 1 → 0 at t=1198, and retail raises
    /// `flags.b1_reap4` on slot 1 in the same tick — the tick-top
    /// `sub_57F20` then frees it at t=1199. That single record was the
    /// take's certification wall.
    ///
    /// ⚠ NO CLASS/MODEL GATE HERE, deliberately — unlike
    /// [`Self::mc2_owner_castle_token`], whose CLASS-15 gate stays
    /// (round 135 relaxed it from (15,2) to class 15): that helper
    /// feeds `sub_5F890`'s PIN, which writes retail `word_0x2E_46`, a
    /// lane the port's polymorphic `f26` alias spends on `dword_0x10_16`
    /// for class 10 (conformance.rs's `(10, _) => scratch10` arm). Firing
    /// the PIN through the same stale index would write the BUILDING'S
    /// OCCUPANCY. Retail's `word_0x2E_46 = word_0x30_48 - 1 = -1` on
    /// slot 1 is real (`explain` t=1198 shows `f2e 0 -> -1`) but lands on
    /// a lane the port does not model for class 10 — pair-blind, and NOT
    /// landable through `f26`. When slot 1 is a class-15 record (mc2l8
    /// t=11584: rival 139's (15,0) fireball token) the same write IS
    /// `f26` and does land — 135-2.
    pub(crate) fn mc2_castle_death_token_purge(&mut self, c: usize) {
        if no_mc2_castle_death_token_purge() {
            return;
        }
        // `if (!a1x->dword_0x10_16)` (EF:61645) — the castle that just
        // took its LAST level — and `Entities[castle->id]->model == 1`
        // (EF:61647): a RIVAL wizard owner. The human's carpet is
        // class-3 model **0** and falls to the `else` arm
        // (`sub_5F890(a1x, 0)`), which the dispatch site already runs.
        if self.g.ent.get(c).is_none_or(|e| e.f26 > 0) {
            return;
        }
        let own = self.g.ent[c].id24;
        if self.g.is_human_tag(own)
            || !self
                .g
                .ent
                .get(own as usize)
                .is_some_and(|o| o.class64 == 3 && o.model65 == 1)
        {
            return;
        }
        // `terrain_2FECE.byte_0x2FED2 & 4` — the level-graphics byte's
        // bit 2. Set on exactly two shipped MC2 levels (022 `gfx_type`
        // 4 and 062 `gfx_type` 7); every other level, mc2l0/l3/l6/l24
        // included, carries 0, so this arm is inert off level 22 by
        // construction.
        if !self.mc2_castle_purge_level() {
            return;
        }
        let Some(ri) = (0..self.mc2_rivals.len())
            .find(|&r| self.mc2_rivals[r].ent != 0 && self.mc2_rivals[r].ent == own)
        else {
            return;
        };
        let m = self.mc2_rivals[ri].book.ent[2] as usize;
        if m == 0 || m >= self.g.ent.len() {
            return;
        }
        self.g.ent[m].flags |= 0x400;
        self.mc2_rivals[ri].book.ent[2] = 0;
    }

    /// `terrain_2FECE.byte_0x2FED2 & 4` (EF:61650; NETHERW.EXE file
    /// `0x84f18` `test byte [eax+0x2fed2],4` / `je 0x84f61`) — the
    /// level-graphics byte's bit 2, plumbed as
    /// [`World::set_mc2_castle_purge_level`] from the level header's
    /// `gfx_type`. Set on exactly two shipped MC2 levels (022
    /// `gfx_type` 4 and 062 `gfx_type` 7); every other level —
    /// mc2l0/l3/l6/l24 included — carries 0, so this arm is INERT off
    /// 022/062 by construction.
    ///
    /// ⚠ This was a stub returning `true` when the purge was first
    /// banked, which is the whole reason the purge measured
    /// net-negative: it fired on mc2l6-rsg (level 6, `gfx_type` 0),
    /// where retail's gate is clear. `MGC_MC2_PURGE_ANY_LEVEL=1`
    /// restores the stub for A/B.
    fn mc2_castle_purge_level(&self) -> bool {
        static ANY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *ANY.get_or_init(|| std::env::var_os("MGC_MC2_PURGE_ANY_LEVEL").is_some()) {
            return true;
        }
        self.mc2_castle_purge_level
    }

    /// `Entities_EA3E4[own]->dword_0xA4_164x->str_611.SpellsEnabled[2]`
    /// (EF:61036 / EF:61652 / EF:61697) — the castle-spell
    /// manifestation of an arbitrary castle OWNER. The out-of-pool
    /// human reads the world-side `mc2_book`; every other wizard reads
    /// its own [`Mc2Spellbook`].
    ///
    /// ⭐⭐⭐ THE GATE IS **CLASS 15**, NOT (15,2) — round 135, mc2l8's
    /// last head (t=11585). Retail has NO gate: `sub_5F890` derefs
    /// `Entities[SpellsEnabled[2]]` raw, and a DEAD wizard's book holds
    /// the boolean **1** marker in every slot it knew (`sub_5E310`
    /// EF:60146, [`World::mc2_scatter_spells`] / the rival twin). So a
    /// dead rival's castle taking its last level (`sub_605E0`,
    /// EF:61643 `sub_5F890(a1x, 1)`) pins POOL SLOT 1 — whatever sits
    /// there — at ITS `word_0x30_48 - 1`; and the rival-owner arm at
    /// level 0 never releases it (EF:61645-58 takes the terrain-flag
    /// branch). mc2l8 t=11584: rival 148 is a corpse (action 3, life
    /// -578), its castle 257 falls 1 → 0, and slot 1 — rival 139's
    /// live (15,0) fireball token, `word_0x30_48 = 5` — reads
    /// `word_0x2E_46` 0 → **4**, then counts 3, 2, 1, 0 over the next
    /// four ticks while rival 139's regen freezes at 16440 (the
    /// armed-token `sub_68DE0` regen pin). The port's (15,2)-only gate
    /// dropped the write; the census could not see it (class-15 `f2e`
    /// is ungraded) and surfaced it one tick later as `(3,1) slot 139
    /// mana` +100.
    ///
    /// For ANY class-15 record the port's `f26` IS retail's
    /// `word_0x2E_46`, so the pin is representable and lands. A
    /// marker that resolves to a NON-class-15 record (mc2l22's (10,45)
    /// building at slot 1: retail writes -1 there) stays unlanded —
    /// the port aliases class 10's `f26` to `dword_0x10_16`
    /// (`scratch10`), a different retail field — and is registered as
    /// pair-blind on [`Self::mc2_castle_death_token_purge`].
    ///
    /// The class gate still matters under import — a book slot that
    /// survived a re-import can name a record that is no longer a
    /// manifestation at all.
    pub(crate) fn mc2_owner_castle_token(&self, own: u16) -> Option<usize> {
        let m = if let Some(hx) = self.hx_of_tag(own) {
            self.mc2_book[hx].ent[2] as usize
        } else {
            let ri = (0..self.mc2_rivals.len())
                .find(|&r| self.mc2_rivals[r].ent != 0 && self.mc2_rivals[r].ent == own)?;
            self.mc2_rivals[ri].book.ent[2] as usize
        };
        (m != 0
            && m < self.g.ent.len()
            && self.g.ent[m].class64 == 15
            && (self.g.ent[m].model65 == 2 || !no_mc2_marker_index_pin()))
        .then_some(m)
    }

    /// [`Self::mc2_owner_castle_token`] with retail's OWN guard and
    /// nothing else — `SpellEnabled[2] != 0` (`sub_5F890` 0x840AF).
    /// The class test above is the port's; retail dereferences the
    /// index raw, which is how a marker `1` reaches pool slot 1.
    pub(crate) fn mc2_owner_castle_token_raw(&self, own: u16) -> Option<usize> {
        let m = if let Some(hx) = self.hx_of_tag(own) {
            self.mc2_book[hx].ent[2] as usize
        } else {
            let ri = (0..self.mc2_rivals.len())
                .find(|&r| self.mc2_rivals[r].ent != 0 && self.mc2_rivals[r].ent == own)?;
            self.mc2_rivals[ri].book.ent[2] as usize
        };
        (m != 0 && m < self.g.ent.len()).then_some(m)
    }

    /// Retail's `word_0x30_48` for a record the port did not home as a
    /// class-15 manifestation — the mirror of `port_ent_lanes_mc2`'s
    /// `f30` arm (`f28` on class 15, else `f50`, and NO home at all on
    /// the (5,10) pyramid / (5,27) hydra / (10,75) wind node, whose
    /// `f50` is spent on another retail word and whose `@0x30` is dead
    /// 0 for the whole take).
    fn mc2_raw_at_30(&self, m: usize) -> i16 {
        let e = &self.g.ent[m];
        match raw_at_30_home(e.class64, e.model65) {
            Raw30Home::F28 => e.f28 as i16,
            Raw30Home::F50 => e.f50,
            Raw30Home::Dead => 0,
        }
    }

    /// Write retail's `word_0x2E_46` into the port's home for THAT
    /// class — the mirror of `port_ent_lanes_mc2`'s `f2e` arm. The
    /// models whose `f46` is repurposed (the (10,{39,57}) sphere, the
    /// (10,45) link and (10,78) mine's `@0x3D`, the (3,{0,1}) pieces,
    /// the (10,{76,77}) orb breathe) publish `—` on that lane and have
    /// no home for `@0x2E` at all, so the write is dropped rather than
    /// allowed to corrupt a live field; the (3,2) castle's `@0x2E`
    /// lives in the `u8` `f59` and cannot hold `-1`. None of those five
    /// families appears on the measured census for this lane.
    fn mc2_write_raw_at_2e(&mut self, m: usize, v: i16) {
        match raw_at_2e_home(self.g.ent[m].class64, self.g.ent[m].model65) {
            Raw2eHome::F26 => self.g.ent[m].f26 = v,
            Raw2eHome::Lease => self.g.ent[m].lease2e.0 = v,
            Raw2eHome::F46 => self.g.ent[m].f46 = v,
            Raw2eHome::None => {}
        }
    }

    /// [`Self::mc2_castle_lock_release`]'s rival twin — the same
    /// `sub_5F890` `a2 == 0` arm priced through the rival column's
    /// `SetSpell` ([`World::mc2_rival_set_spell`]) against the rival's
    /// OWN castle. No player-effect teardown: spell 2 has no arm in
    /// [`Self::mc2_cast_expire`]'s match, and every effect that one
    /// drops is a human-carpet lane.
    fn mc2_rival_castle_lock_release(&mut self, m: usize, own: u16) {
        self.g.ent[m].f26 = 0;
        // ⚠ RETAIL'S `sub_6D880` HERE TAKES THE **CASTLE**, NOT `m` —
        // NETHERW.EXE file 0x840BC-0x840CB, quoted in the entry arm of
        // [`Self::mc2_castle_spell_tick`]. The human column landed that
        // ONE-TICK LAG ([`no_mc2_castle_tier_defer_lag`]); the rival
        // twin kept applying the deferred tier in the release's own
        // tick, so a rival castle's settle re-priced its (15,2) token
        // one tick early and the import then had nothing left to run.
        // EF:61036 indexes `SpellsEnabled[2]` with no player test, so
        // ONE law covers both columns.
        if no_mc2_rival_castle_tier_defer_lag() && self.g.ent[m].f44 > 0 {
            let t = (self.g.ent[m].f44 - 1) as u8;
            self.g.ent[m].f44 = 0;
            self.mc2_rival_set_spell(m, t, own);
        }
        if self.rival_castle(own).is_some() {
            let tier = self.g.ent[m].f71;
            self.mc2_rival_set_spell(m, tier, own);
        }
    }

    /// Is the human's castle-spell UPGRADE LOCK engaged? — true while a
    /// human-owned castle build ball is in flight, or the human's castle
    /// is in any non-standing-idle transform state (build/upgrade/
    /// downgrade/settle). Mirrors where retail calls `sub_5F890(*,1)`
    /// (throughout the transform) vs `(*,0)` (return to the standing
    /// action-4 idle).
    /// Did the cast actually put a (9,10) castle ball in the air?
    /// Retail asks it as `test %eax,%eax` on `NewEvent`'s return
    /// (VA 0x69BAE's guard); the port has no spawn handle to test, so
    /// it reads the pool the ball has just been placed in.
    fn mc2_castle_ball_aloft(&self, hx: Hx) -> bool {
        let tag = self.g.human_tag[hx].0;
        self.g
            .ent
            .iter()
            .skip(1)
            .any(|e| e.class64 == 9 && e.model65 == 10 && e.id24 == tag && e.flags & 0x400 == 0)
    }

    fn mc2_castle_lock_active(&self, hx: Hx) -> bool {
        // The cast in transit: the (9,10) castle ball still flying.
        if self.mc2_castle_ball_aloft(hx) {
            return true;
        }
        // The castle mid-transform: idle = action 4, no settle timer,
        // no armed upgrade (`mc2_castle_standing`/`_build`/`_destroy`).
        if let Some(c) = self.seat_castle(hx) {
            let e = &self.g.ent[c];
            let idle = e.tick70 == 4 && e.f50 == 0 && e.flags & super::castle::F_UPGRADE_ARMED == 0;
            !idle
        } else {
            false
        }
    }

    /// Cast-window expiry: apply the pending tier (`sub_6D880`
    /// EF:58215) and drop the armed-window player effects.
    pub(crate) fn mc2_cast_expire(&mut self, hx: Hx, spell: usize, m: usize) {
        if self.g.ent[m].f44 > 0 {
            let t = (self.g.ent[m].f44 - 1) as u8;
            self.g.ent[m].f44 = 0;
            self.mc2_set_spell(hx, m, t);
        }
        // Armed-window effects end with the window (shield
        // `dword &= 0xFFBFBFFF` EF:56496-tail, invis `&= 0xDF`
        // EF:57068-tail, and the sight/rebound analogues).
        match spell {
            // `parent.dword &= 0xFFBFBFFF` (EF:56537) clears BOTH
            // stage bits — byte[1] 0x40 and byte[2] 0x40 — in one
            // store, so the window's end drops the ARMED stage too.
            6 => {
                self.set_human_shield(hx, false);
                self.set_human_shield_armed(hx, false);
            }
            8 => {
                self.players[hx].rebound = false;
                self.g.mc2_rebound_precise[hx].0 = 0;
            }
            // ⭐⭐⭐ THE DUEL WINDOW'S END IS NOT THE LOCK'S END, AND
            // THE PORT'S OWN COMMENT SAID SO. Retail never clears
            // `word_0x146_326` from the manifestation's expiry: the
            // only writer of 0 is `sub_5DE30`'s liveness `else`
            // (EF:60343, guard EF:60312), which runs at the CARPET's
            // walk slot (called from `sub_5D530`, EF:60117) —
            // below the manifestation's — so the dead charge is not
            // read until the NEXT tick's enforcement pass. Collapsing
            // it onto the expiry edge cleared the lock one tick early
            // on every window: mc2l6-rsg t=1563 / 1759 / 2110 / 2356 /
            // 2676 / 3076, six boundaries, the port's own
            // pre-clear value equal to retail's each time. See
            // [`crate::engine::world::no_mc2_duel_expire_lag`].
            14 => {
                if crate::engine::world::no_mc2_duel_expire_lag() {
                    self.mc2_duel[hx] = None;
                }
            }
            // Metamorph teardown (`sub_6A030` expiry EF:56394): despawn
            // the pose-puppet, un-hide the carpet, sound 60.
            4 => {
                let c = self.g.ent[m].f146 as usize;
                if c != 0 && c < self.g.ent.len() && self.g.ent[c].class64 == 5 {
                    self.g.ent[c].flags |= 0x400;
                }
                self.g.ent[m].f146 = 0;
                self.players[hx].metamorph = 0;
                // …and the cloak lifts with it: `v2x->byte[0] &= 0xDF`
                // beside the `DisableEntityDrawing04_57F10` despawn
                // (EF:56403; shipped EXE 0x8EA50 `and cl,0xdf`). It is
                // the shared 0x20 bit, so retail's metamorph expiry
                // cancels a live Invisibility cloak too — faithful.
                // `invis_strength` is left alone (retail never touches
                // `byte_0x1BF_447` here). Witness: mc2l0-spells-galore
                // t=8039, the (5,25) puppet at slot 197 is reap-flagged
                // and the carpet's flags go 301 → 269 in the same tick.
                if !crate::mc2::roster::no_metamorph_cloak() {
                    self.set_human_invisible(hx, false);
                    self.g.player_invisible[hx] = self.ghost;
                }
                self.g.snd_player(hx, 60);
            }
            0xB => {
                self.set_human_invisible(hx, false);
                self.players[hx].invis_strength = 0;
            }
            0xC => self.players[hx].beyond_sight = false,
            5 => self.players[hx].heal_active = false,
            3 => {
                self.players[hx].accel = 0;
                self.players[hx].accel_mc2_factor = 0;
            }
            // Teleport's window end repeats the flight target-speed
            // zero (`sub_6AD60` countdown-out arm, EF:57046
            // `speed_0xc_12 = 0` beside the `sub_6D880` teardown).
            0xA => self.pending_speed_zero[hx] = true,
            _ => {}
        }
    }

    // ---- first-tick fire: dispatch + direct effects ------------------------

    /// The `sub_6DCA0` arm table (cast-path trace §2/§2.1): spell →
    /// (class-9 subtype, impact class/model, payload/charge flags).
    /// The charged variants (fireball 28/(10,76), thunder 12/(9,9))
    /// key on the tier's `life_0x1A`.
    pub(crate) fn mc2_dispatch_arm(spell: usize, life: i8) -> Option<DispatchArm> {
        let arm = |subtype, impact, charge| {
            Some(DispatchArm {
                subtype,
                impact,
                charge,
            })
        };
        match spell {
            0 if life >= 2 => arm(28, (10, 76), false),
            0 => arm(0, (10, 0), false),
            // Lightning L1/L2 (subtype 12): the bolt's OWN
            // `byte_0x43_67`/`byte_0x44_68` really are `(9,9)` —
            // measured, mc2l22 t=1080 slot 485. The `(10,38)` storm is
            // `sub_66FD0`'s HARD-CODED detonation, not a descriptor
            // (see `mc2_proj_impact`), and the `(9,9)` pair is what the
            // storm CHAINS onto the beams it rains (`sub_35640`
            // EF:25919-24 spawns each as `_4A190(pos, storm->b43,
            // storm->b44)`). Stamping `(10,38)` on the BOLT put a value
            // there retail never holds, and left the impact unable to
            // find its own effect on any bolt the pair lane IMPORTS —
            // 82 storm births in mc2l22, zero minted by the port.
            // ⭐⭐ A DELIBERATE APPROXIMATION HAS AN EXPIRY DATE: the
            // "(10,38) internals untraced" this shortcut cited have
            // since landed (`mc2_spawn_lightning_burst`/`mc2_storm_tick`).
            7 if matches!(life, 1 | 2) => arm(12, (9, 9), false),
            7 => arm(9, (10, 23), false),
            9 => arm(3, (10, 17), true),
            15 => arm(23, (10, 71), true),
            16 => arm(5, (10, 11), true),
            17 => arm(2, (10, 15), true),
            18 => arm(4, (10, 9), true),
            20 => arm(22, (10, 67), true),
            // Steal Mana (13): a class-9 subtype-8 homing bolt whose
            // impact is the (10,25) "steal" burst (`sub_6B3E0` →
            // `sub_6DCA0(…,0xD,…)`, EF:57195; docs/spell-audit/
            // steal-mana.md). The bolt carries the tier's `sub_spell`
            // in f44 (2000/4000/10) — the drain amount the (10,25)
            // impact stamps into the struck wizard's ch3 inbox.
            13 => arm(8, (10, 25), false),
            21 => arm(26, (10, 22), true),
            25 => arm(30, (10, 89), true),
            _ => None,
        }
    }

    /// First cast tick — spawn the spell's effect. Projectile spells
    /// route through the `sub_6DCA0` dispatch; the direct-effect
    /// spells (cast-path trace §2.2) write player state or spawn
    /// their entity directly.
    fn mc2_spell_fire(&mut self, hx: Hx, spell: usize, m: usize, p: PlayerPose, ctx: &MobCtx) {
        let tier = self.g.ent[m].f71 as usize;
        let row = self.g.assets.spells.get(spell).copied();
        let mut sub = row.map_or(Mc2SubSpell::default(), |r| r.tiers[tier.min(2)]);
        // The 0x15/0x19 arms divide the payload by the charge
        // (`subSpellIndex_2 / life_0x1A` when charged, EF:44189-219).
        if matches!(spell, 21 | 25) && sub.life > 0 {
            sub.sub_spell /= sub.life as i32;
        }

        // The projectile band (10 spells → sub_6DCA0, EF:44020).
        if let Some(arm) = Self::mc2_dispatch_arm(spell, sub.life) {
            // Cast sound v6: fireball 9, thunder charged 9 /
            // uncharged 23, default 15 (EF:44233 + §2 table).
            let v6 = match spell {
                0 => 9,
                7 if matches!(sub.life, 1 | 2) => 9,
                7 => 23,
                _ => 15,
            };
            // Lightning T3 (`life_0x1A == 2`): the cast site
            // `sub_6A5C0` loops `(life != 1) + 1` spawns, fanning
            // the pair's yaw ±113 (≈±19.9°) off the aim heading
            // (EF:56599-56656) — "two L2 bolts side by side". The
            // twins cross-link via f54 (word_0x34_52, EF:57002-06);
            // retail's only consumer is the beacon drone-lock
            // despawn arm (sub_66FD0, `Gen::mc2_m12_beacon_boost`,
            // round 150): the twin that locks the mine first frees
            // the other through this link. The
            // cast sound rides EACH spawn (sub_6DCA0 tail,
            // EF:44224-33), and the loop tolerates a full pool.
            let fan: &[u16] = if spell == 7 && sub.life == 2 {
                &[113, 113u16.wrapping_neg()]
            } else {
                &[0]
            };
            let mut twin: Option<usize> = None;
            for &off in fan {
                let Some(i) = self.mc2_launch(hx, spell, m, &arm, sub, p) else {
                    continue;
                };
                // Steal Mana: `sub_6B3E0` post-writes the bolt's
                // `byte_0x46_70` = the TOKEN's tier index (EF:57213);
                // the `sub_6DCA0` 0xD arm never touches it. The
                // (10,25) burst copies it (EF:63559) and the drain
                // indexes `SPELLS[13]` by it. Not `sub.life` (0/0/1).
                // mc2l6-rsg t=8673 slot 736: the human's L2 bolt reads
                // b46 = 1.
                if spell == 13 {
                    self.g.ent[i].f71 = tier as u8;
                }
                if off != 0 {
                    let yaw = p.heading.wrapping_add(off) & 0x7FF;
                    let e = &mut self.g.ent[i];
                    e.f30 = yaw;
                    // ⭐ THE FAN OFFSET LANDS ON `yaw_0x1C_28` ALONE —
                    // `sub_6A5C0` EF:56629-42 computes `v7 = yaw ±113`
                    // and stores it to `yaw_0x1C_28`; the `axis_0x9A`
                    // step above it (EF:56622-27) took the UNFANNED
                    // bearing, and the arm writes no `roll_0x20_32`
                    // at all. See [`no_launch_roll_absence`].
                    if no_launch_roll_absence() {
                        e.f34 = yaw;
                    }
                }
                if let Some(t) = twin {
                    // ⭐⭐⭐ THE CROSS-LINK'S HOME IS **f54** (@0x34),
                    // NOT f52 (@0x32) — see [`no_mc2_m12_handle_home`]
                    // for the citation, the shipped bytes and the
                    // 177+177-row census. The old `f52` write put the
                    // handle in `word_0x32_50`, which retail leaves at
                    // 0 on every (9,12).
                    if no_mc2_m12_handle_home() {
                        self.g.ent[i].f52 = t as u16;
                        self.g.ent[t].f52 = i as u16;
                    } else {
                        self.g.ent[i].f54 = t as u16;
                        self.g.ent[t].f54 = i as u16;
                    }
                }
                twin = Some(i);
                self.g.snd_player(hx, v6);
            }
            return;
        }

        // The direct-effect band (§2.2).
        match spell {
            // posses (`sub_69640` EF:55915), sound 40. The tier gate is
            // the SUBSPELL's `life_0x1A`, and it picks a different
            // ENTITY, not just a different payload (EF:55946-49):
            //
            //   life 0    → `sub_69900` (EF:56039) spawns the BASIC
            //               **(9,1)** bolt, impact (10,12);
            //   life 1..3 → the inline arm spawns **(9,17)**
            //               (EF:55950), `byte_0x44_68` = 54 (life 1) /
            //               69 (life 2) / the NewEvent 0 (life 3);
            //   life > 3  → the `<= 3` gate fails: NOTHING is cast.
            //
            // Per-tier delivery (docs/spell-audit/possession.md): T0
            // plain claim `(10,12)`; T1 Mana Magnet — claim + the
            // `(10,54)` attract aura (range 15); T2 Mana Lock — FORCED
            // claim ((10,70) steal pulse) + the `(10,69)` aura
            // (range 20).
            //
            // Row 1's `life` column IS (0,1,2) on the baked CD, so the
            // tier index stands in when there is no SPELLS row at all
            // (unit fixtures) — the port used to key on the tier index
            // ALONE and always launched (9,17): full-take mc2l24 read
            // (9,1) 362 missing / 0 extra.
            1 => {
                let tier = self.g.ent[m].f71 as usize;
                let life = row.map_or(tier as i8, |_| sub.life);
                let arm = match life {
                    0 => Some((1u8, (10u8, 12u8))),
                    1 => Some((17, (10, 54))),
                    2 => Some((17, (10, 69))),
                    3 => Some((17, (10, 0))),
                    _ => None,
                };
                if let Some((subtype, impact)) = arm {
                    self.mc2_possess_launch(hx, m, p, subtype, impact, sub);
                }
            }
            // castle: the castle-ball cast (the MC1 machinery on the
            // MC2 column — the sub_69AB0 build queue is the castle
            // column's banked follow-up), sound 15.
            2 => {
                // The hand pick feeds MC1's muzzle anchor only; the
                // MC2 lane always spawns at the carpet (cast_castle's
                // mc2 gate), so the side is inert here.
                self.cast_castle(hx, p, false, Some(m));
                self.g.snd_player(hx, 15);
            }
            // speed_up: the accelerate channel (`GetScroll_69DB0`
            // EF:56189), sound 19. The per-tier factor `subSpellIndex`
            // = {2,3,4} drives 160/240/320 sustained (not the MC1
            // fixed 3.0/2.0) — docs/spell-audit/speed.md. MC2's one
            // spell doubles as MC1's Accelerate AND Accelerate
            // Backwards: the direction is the caster's CURRENT
            // COMMAND sign (`v2 = speed_0xc_12 >= 0 ? 1 : -1`,
            // standstill counts as forward) — the FLIGHT BLOCK's
            // `+0xC`, NOT the entity's `actSpeed_0x82_130`
            // (`NETHERW.EXE` 0x8E5F8-0x8E603; see
            // [`no_mc2_speed_sign_cmd`]). Retail re-derives it every
            // effect tick, but the hard speed override makes the sign
            // self-sustaining, so the cast-time latch is the same
            // law.
            3 => {
                let dir_src = if no_mc2_speed_sign_cmd() {
                    p.speed
                } else {
                    self.mc2_cmd_speed[hx]
                };
                self.players[hx].accel = if dir_src >= 0 { 1 } else { -1 };
                self.players[hx].accel_held = true;
                self.players[hx].accel_mc2_factor = sub.sub_spell.clamp(1, 8) as i8;
                self.mc2_award_xp(self.g.human_tag[hx].0, 3, 1);
                self.g.snd_player(hx, 19);
            }
            // heal (5) NEVER REACHES HERE. `sub_6A300` is not a
            // "fire once on the first tick" spell at all — its whole
            // body, sound 25 and the XP award included, is the
            // per-tick effect state, and the XP is gated on actually
            // healing. See [`Self::mc2_heal_token_tick`], which
            // `mc2_manifestation_tick` early-returns into.
            5 => debug_assert!(false, "mc2 heal routes through mc2_heal_token_tick"),
            // shield (EF:56496): armed-window flag — and WHICH flag is
            // the TIER's `life_0x1A`, exactly as the rebound arm below
            // reads it. `life == 0` (Shield I/II) is the CHARGED bit
            // `byte[1] |= 0x40`; `life == 1` (Shield III) is the
            // one-shot ARMED bit `byte[2] |= 0x40`, whose absorb NULLS
            // the letter and promotes to CHARGED. This arm is the
            // FIRST tick of the token's own window, so it stands in
            // for `sub_6A480`'s first pass; the per-tick CHARGED
            // re-stamp lives in `mc2_manifestation_tick`.
            6 => {
                if sub.life == 1 && !no_mc2_shield_armed() {
                    self.set_human_shield_armed(hx, true);
                } else {
                    self.set_human_shield(hx, true);
                }
                // ⛔ NO XP HERE. `sub_6A480` never calls `sub_6D8B0`;
                // spell 6's only award site in the shipped image is
                // the ABSORB gate in `sub_5EFA0` — see
                // [`no_mc2_shield_xp_on_absorb`] and its twin hunk in
                // `World::apply_player_damage`.
                if no_mc2_shield_xp_on_absorb() {
                    self.mc2_award_xp(self.g.human_tag[hx].0, 6, 1);
                }
            }
            // rebound (`sub_6AA00` EF:56721-51): armed-window flag +
            // the tier's LAW bit — `life==1` (T3) stamps PRECISE
            // (byte0xc[0]|=0x10: exact return down the reverse ray,
            // doubled payload), `life==0` scatter (byte[1]|=0x80).
            // Durations ride the table (125/251/125); the deflection
            // itself lives in `mc2_rebound_deflect` at the movers'
            // victim-hit gates.
            8 => {
                // The window flag itself is NOT a first-tick latch —
                // `sub_6AA00` re-stamps it at the TOKEN's own walk
                // slot every afforded tick, which is what lets the arm
                // frame deflect. See `mc2_manifestation_tick`.
                //
                // ⛔ AND NEITHER IS THE XP — see
                // [`no_mc2_rebound_xp_on_deflect`]. `sub_6AA00` has no
                // `sub_6D8B0` call at all; spell 8's only award site
                // is the DEFLECT in `sub_68740`, which
                // `Gen::mc2_rebound_deflect` already mails. This arm
                // was a second, invented credit.
                if no_mc2_rebound_xp_on_deflect() {
                    self.mc2_award_xp(self.g.human_tag[hx].0, 8, 1);
                }
            }
            // teleport (`sub_6AD60` EF:56860): the real per-tier
            // relocation — to own castle / save+return toggle / cycle
            // all castles (docs/spell-audit/teleport.md). Sound 22 is
            // played inside on a castle success (silent random hop).
            0xA => {
                self.mc2_cast_teleport(hx, m, p);
                self.mc2_award_xp(self.g.human_tag[hx].0, 10, 1);
            }
            // invisible (EF:57068): set the flag AND the per-tier
            // break strength (`byte_0x1BF_447 = life_0x1A` = {1,2,3}),
            // which the arm-path break law consults (mc2_cast_gate).
            0xB => {
                self.set_human_invisible(hx, true);
                self.players[hx].invis_strength = sub.life.clamp(0, 3) as i8;
                // …and the SAME statement block clears the caster's
                // spawn grace (`word_0x159_345 = 0`, NETHERW.EXE
                // 0x8FA46) — see [`no_mc2_invis_clears_grace`].
                if !no_mc2_invis_clears_grace() {
                    self.players[hx].grace = 0;
                }
                self.mc2_award_xp(self.g.human_tag[hx].0, 11, 1);
            }
            // beyond_sight (EF:57132).
            0xC => {
                self.players[hx].beyond_sight = true;
                self.mc2_award_xp(self.g.human_tag[hx].0, 12, 1);
            }
            // summon_army (`sub_6C170` EF:57638): the (9,24) carrier
            // flies forward and LANDS to spawn a ring of allied class-5
            // creatures. Impact (10,72); charge=true carries the tier's
            // creature MODEL (life = 19/2/25/16) in f71 (the ring's army
            // size + model). Sound 9 (docs/spell-audit/summon-creatures.md).
            0x13 => {
                if self
                    .mc2_launch(
                        hx,
                        spell,
                        m,
                        &DispatchArm {
                            subtype: 24,
                            impact: (10, 72),
                            charge: true,
                        },
                        sub,
                        p,
                    )
                    .is_some()
                {
                    self.g.snd_player(hx, 9);
                }
            }
            // fools_mana (`sub_6C870` EF:57868): a SHOTGUN of six
            // neutral fake-mana decoys, each a trap that detonates on
            // an enemy's possession claim. Cast sound 11 once after
            // the burst (docs/spell-audit/fools-mana.md).
            0x16 => {
                // Retail's sub_6C870 cast awards no XP (the trap's
                // SPEND points in sub_36680 do); sound 11 gates on the
                // burst spawning (EF:57924).
                if self.mc2_cast_fools_mana(hx, m, p, sub) {
                    self.g.snd_player(hx, 11);
                }
            }
            // magic_mine (`sub_6CAC0` EF:57960): the (9,29) carrier flies
            // forward and LANDS to place a persistent (10,78) proximity
            // mine. Impact (10,78); charge=true so the tier rides f71
            // (blast intensity) while f44 carries the tier lifespan
            // (subSpell). Sound 15 (docs/spell-audit/magic-mine.md).
            0x17 => {
                if self
                    .mc2_launch(
                        hx,
                        spell,
                        m,
                        &DispatchArm {
                            subtype: 29,
                            impact: (10, 78),
                            charge: true,
                        },
                        sub,
                        p,
                    )
                    .is_some()
                {
                    self.g.snd_player(hx, 15);
                }
            }
            // alliance: class-9 subtype 25 direct (`sub_6CD20`
            // EF:58039), sound 9. Impact = the (10,74) CONVERSION
            // executor (NOT a fire — it allies the target rather than
            // burning it). charge=true carries the tier's area radius
            // in f71 (life = 16/26/32 tiles); f44 already rides the
            // tier's subSpell (610/1100/2710) = the charm DURATION,
            // not damage.
            0x18 => {
                if self
                    .mc2_launch(
                        hx,
                        spell,
                        m,
                        &DispatchArm {
                            subtype: 25,
                            impact: (10, 74),
                            charge: true,
                        },
                        sub,
                        p,
                    )
                    .is_some()
                {
                    self.g.snd_player(hx, 9);
                }
            }
            // metamorph (`sub_6A030` EF:56294): transform the caster
            // into a pooled class-5 creature (pose-puppet), carpet hidden.
            4 => self.mc2_cast_metamorph(hx, m, sub, p),
            // duel (`sub_6B610` EF:57258): FIRE THE (9,7) DUEL DART.
            // The (10,26) tether is the dart's IMPACT, not the cast
            // product. The grip → lock → enforcement machinery lives
            // in world.rs (`mc2_duel_tether_tick` / `mc2_duel_enforce`);
            // docs/spell-audit/duel.md.
            0xE => self.mc2_cast_duel(hx, m, sub, p),
            _ => {}
        }
        let _ = ctx;
    }

    /// ONE possession bolt — `sub_69900` (EF:56039) for the basic
    /// `(9,1)` arm, `sub_69640`'s inline block (EF:55950-79) for the
    /// leveled `(9,17)`. The two write the same lanes, so the launch
    /// tail below is shared; only the subtype/impact pair differs.
    /// Split out because the HELD RE-FIRE reaches the basic arm
    /// directly, past the tier walk (see `mc2_manifestation_tick`).
    fn mc2_possess_launch(
        &mut self,
        hx: Hx,
        m: usize,
        p: PlayerPose,
        subtype: u8,
        impact: (u8, u8),
        sub: Mc2SubSpell,
    ) {
        let Some(i) = self.mc2_launch(
            hx,
            1,
            m,
            &DispatchArm {
                subtype,
                impact,
                charge: false,
            },
            sub,
            p,
        ) else {
            return;
        };
        // `sub_69900`'s launch tail (EF:56050-67) — the (9,17) arm
        // writes the same lanes (EF:55956/55966/55968), so both
        // share it:
        //   `mana_0x90_144` = the TOKEN's mana — now the universal
        //     `mc2_launch` copy (the l24 corpus records 33),
        //   `dword_0x10_16` = 200 on the basic bolt (@0x10 → f26);
        //     the leveled arm instead squares the token's
        //     `subSpellIndex << 8`.
        // The `position.z += caster fov` of EF:56054 / EF:55969 is
        // already carried by `muzzle`, which launches at pose z +
        // PLAYER_HH.
        // DELIBERATE: retail also stamps `word_0x26_38` = the token's
        // SLOT (@0x26 → f40), but the port spends f40 on the spell
        // INDEX — the impact XP back-ref (`mc2_proj_impact`), which
        // retail hard-codes per handler (`sub_6D8B0(id, 1, 1)`,
        // EF:63314/59052). The lane is not compared; the XP wiring
        // wins.
        let token_sub = self.g.ent[m].f30 as i32;
        let caster_speed = self.mc2_caster_act_speed(hx, p);
        {
            let e = &mut self.g.ent[i];
            // ⭐⭐⭐ THE LEVELED BOLT'S @0x10 IS A **32-BIT SQUARED
            // REACH** AND THE PORT'S `f26` IS AN i16 — so the honest
            // formula truncated to zero. `sub_69640` (EF:56329) is
            // `v4x->dword_0x10_16 = (a1x->subSpellIndex_0x2A_42 << 8)
            //  * (a1x->subSpellIndex_0x2A_42 << 8);` — the token's
            // @0x2A is a TILE radius (15 on the corpus), so the word
            // is `(15 << 8)²` = **14,745,600**, and `x as i16` of that
            // is 0 (0xE10000 & 0xFFFF). The port therefore wrote 0
            // into every leveled possession bolt.
            // Home the ROOT and publish the square — the EXACT shape
            // round 148 gave the (10,54)/(10,69) aura, whose @0x10 is
            // the same `(k << 8)²` encoding (`import_ent_mc2`'s
            // `isqrt(scratch10) >> 8` arm and `port_ent_lanes_mc2`'s
            // squaring arm; the aura's own `f26` seat in
            // `mc2_proj_impact` is literally `if fm == 54 { 15 } else
            // { 20 }`). Nothing in the port reads a class-9 `f26`, so
            // this is a HOME, not a behaviour change.
            // WITNESS: `(9,17) scratch10` 5,944 rows over 12 takes,
            // retail **14745600** / port 0 (mc2l6-rsg 58 rows t=3543
            // slot 807; mc2l11 448 rows; mc2l17 91).
            // `MGC_NO_MC2_POSSES_REACH_ROOT=1` restores the truncation.
            e.f26 = if subtype == 1 {
                200
            } else if no_posses_reach_root() {
                let v = token_sub << 8;
                (v.wrapping_mul(v)) as i16
            } else {
                token_sub as i16
            };
            // BOTH possession arms take the carpet boost RAW —
            // `v2x->actSpeed += a2x->actSpeed` (EF:56048 / EF:55953)
            // with no clamp. The [384, 0x2000] clamp `mc2_launch`
            // applies is `sub_6DCA0`'s alone (EF:44226-31), and it
            // both floors a REVERSING carpet's bolt at 384 and drops
            // the negative term outright. mc2l4 t=13 slot 303 records
            // speed **336** = 384 − 48 on a backing carpet.
            e.f126 = 384i32.saturating_add(caster_speed as i32) as i16;
        }
        // Sound 40 only on a successful spawn.
        self.g.snd_player(hx, 40);
    }

    /// The launch block shared by every projectile arm (cast-path
    /// trace §1.5, EF:55853-55886): spawn at the caster, owner id,
    /// payload, muzzle height, launch angles from the carpet pose,
    /// speed boost from the caster's flight speed (clamped 384..
    /// 0x2000 — EF:44226-31), and the local-player muzzle sprite 42.
    /// `sub_6C870` (EF:57868) — Fool's Mana: throw SIX neutral
    /// FAKE-mana spheres from the caster's hand in a ±85 yaw cone. Each
    /// carries a random mana value (the disguise) but is a TRAP — the
    /// retail homes verbatim: parentId (id24) = caster (EF:57905), tier
    /// `byte_0x46_70` (f71) = `life` (EF:57907), damage payload
    /// `subSpellIndex_0x2A_42` (f44) = `subSpellIndex_2` (EF:57906),
    /// colour neutral `playerEntityIndex` (f144) = 0 (EF:57908). When a
    /// NON-owner possession claims one, the sphere's tick springs the
    /// tier retaliation (mc1/combat.rs `ball_tick` →
    /// [`Gen::mc2_fools_retaliate`]) instead of handing over the mana
    /// (docs/spell-audit/fools-mana.md). The trap machinery is the
    /// (10,57) TICK's, not a cast flag: the authored ground spheres run
    /// the identical path off their NewEvent defaults.
    fn mc2_cast_fools_mana(&mut self, hx: Hx, m: usize, p: PlayerPose, sub: Mc2SubSpell) -> bool {
        // ⭐⭐⭐ THE HUMAN'S FOOL'S MANA IS `sub_6C870` — THE SAME BODY
        // THE RIVAL COLUMN RUNS. The port's human arm was a paraphrase
        // assembled from the WRONG retail function: the `dword_0x10_16
        // = wizext->byte_0x154; … = 0` pair it cited as EF:57825-26
        // belongs to `sub_6C7B0` (the class-9 subtype-0x15 launcher a
        // dozen lines above, EF:57810-31), not to `sub_6C870`, which
        // never touches @0x10 at all. Statement by statement against
        // EF:57888-57922 the paraphrase differed in six places:
        //
        //   * `axis_0x9A` (dest) — INVENTED. Retail writes no velocity
        //     accumulator here; the sphere is a THROWN body carried by
        //     `actSpeed` through `sub_35FB0`'s flight arm
        //     (EF:26457-26524, `Gen::mc2_fool_flight`). The port's
        //     96-unit polar nudge is the settle arm's lane.
        //   * `actSpeed_0x82_130` — MISSING. EF:57897-903:
        //     `v3 = 4*caster.actSpeed; clamp(140, 280);
        //      actSpeed = (tokenLCG & 0x7F) + v3`.
        //   * `pitch_0x1E_30` — MISSING (EF:57919); the flight arm's
        //     `MoveEntity(yaw, pitch, actSpeed)` reads it, so a
        //     pitchless sphere flies flat.
        //   * the LCG is the TOKEN's `a1x->rand_0x14_20` (EF:57902,
        //     EF:57914) — the port drew off the SPHERE, which both
        //     desynchronises the fan and burns a different seed.
        //     TWO draws per sphere, speed first then yaw.
        //   * the spawn z is the CASTER's, raised by the hand muzzle
        //     (`_4A190(&caster.position, …)` EF:57894 then
        //     `sub_68E50` EF:57904) — the port snapped it to the
        //     GROUND, so every decoy started buried at the player's
        //     feet.
        //   * tier ≥ 3 colours the sphere with the caster
        //     (EF:57909-12); the port left every tier neutral.
        //
        // The free-slot gate is retail's too (`sub_4A810_get_0x35plus()
        // > 6`, EF:57888): the burst is all-or-nothing.
        if self.g.free.len() <= 6 {
            return false;
        }
        // ⭐ NO MUZZLE LIFT — THE SECOND CALL PATH. `sub_6C870`
        // spawns at `&<caster>->position_0x4C_76` (EF:57894) and runs
        // `sub_68E50` (EF:57904), which steps 256 units LATERALLY at
        // pitch 0 and never writes z: the body carries no
        // `position_0x4C_76.z += array_0x52_82.fov` at all, exactly
        // like the army/alliance arms in `mc2_launch`. `muzzle_side`'s
        // z is `p.z + PLAYER_HH`, so every human decoy was born 100
        // units high; its lateral step and terrain revert already run
        // off the UNLIFTED `p.z`, so only the returned height was
        // wrong. The RIVAL funnel (mc2/rivals.rs, `s == 0x16`) has
        // always spawned at the caster's own z. mc2l6-rsg pair
        // 10633->10634: retail births six (10,57) at z
        // 389/401/398/394/390/386 off a caster z of 257; the port held
        // 489/501/498/494/490/486 — the only six rows in the pair.
        let (mx, my, _) = self.muzzle_side(p, self.mc2_fire_side(hx, m));
        let payload = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
        let tier = sub.life.max(0) as u8;
        let base = (4 * self.mc2_caster_act_speed(hx, p) as i32).clamp(140, 280);
        let mut spawned = false;
        for _ in 0..6 {
            let Some(s) = self.g.mc2_spawn_mana_sphere(57, mx, my, p.z) else {
                break;
            };
            // EF:57902-03 — the TOKEN's LCG, drawn for the speed FIRST.
            let r1 = self.g.ent_rand(m);
            let speed = ((r1 & 0x7F) as i32 + base) as i16;
            // EF:57914-18 — the second TOKEN draw, the ±85 yaw fan.
            // (`wizext + 0x18` is the human aim offset; the port keeps
            // no such lane, so it is 0 here as on the rival column.)
            let r2 = self.g.ent_rand(m);
            let yaw = p.heading.wrapping_sub(85).wrapping_add((r2 % 0xAA) as u16) & 0x7FF;
            let e = &mut self.g.ent[s];
            e.f126 = speed; // actSpeed — the throw
            e.id24 = self.g.human_tag[hx].0; // parentId = caster (the skip-gate)
            e.f44 = payload; // subSpellIndex damage payload
            e.f71 = tier; // byte_0x46_70 retaliation tier
            e.f144 = 0; // NEUTRAL — no owner colour (the "fool")
            e.f30 = yaw; // launch heading
            e.f32 = p.pitch; // launch pitch (EF:57919)
            if tier >= 3 {
                e.f144 = self.g.human_tag[hx].0;
                self.g.ball_resize(s);
            }
            spawned = true;
        }
        spawned
    }

    /// `sub_6A030` (EF:56294) — Metamorph: spawn ONE class-5 creature
    /// (model = the tier's `life`: 2 Day / 19 non-Day, 25, 16) at the
    /// caster and mark it a pose-PUPPET (StageVar2/site_z = 12, action
    /// `8*M+7`) allied to the player. The wizard keeps normal control
    /// and casting; the carpet is hidden (`player.metamorph`) and the
    /// creature draws in its place. The manifestation links the creature
    /// (`word_0x96_150` → f146) for teardown at the cast-window expiry
    /// (mc2_cast_expire). Sound 60; XP on the fire tick. No control
    /// rebinding is needed — the creature is slaved to the live player
    /// pose (docs/spell-audit/summon-creatures.md Part A).
    /// `sub_6B610` first-tick body (EF:57289-57316) — THE DUEL DART.
    ///
    /// ⭐⭐⭐ DUEL FIRES A PROJECTILE. Retail's first live tick of the
    /// class-15 model-14 manifestation spawns a **(9,7)** flyer
    /// (`_4A190(&caster.position, 9, 7)` EF:57291 → the `sub_4D740`
    /// creator, EF:34898) whose IMPACT pair is `byte_0x43_67 = 10 /
    /// byte_0x44_68 = 26` — the (10,26) tether is what the dart makes
    /// when it lands, not what the cast makes where you stand. The
    /// port spawned the tether at the caster's feet, which is a
    /// self-grip: every duel gripped whatever stood next to the
    /// CASTER instead of what the dart hit.
    ///
    /// mc2l6-rsg t=1326 is the witness — retail's slot 855 is a live
    /// (9,7) (max_life 21 = 0x2000/384, sprite 213, speed/min_speed
    /// 384, action 7, `b44` 26, mana 51 off the token, `f2a` 5170 =
    /// the tier's range payload) where the port held a (10,26) with
    /// life 8 and speed 16. Subtype 7 was missing from `CREATORS`
    /// outright: it is the ONE class-9 creator no `sub_6DCA0` band arm
    /// reaches (docs/spell-audit/rival-spells.md §3.1 flagged the hole
    /// in 2026-07; this closes it).
    ///
    /// ⚠ NO SPEED BOOST HERE. `sub_6B610` has no
    /// `actSpeed_0x82_130 +=` line at all — neither `sub_6DCA0`'s
    /// clamped `a5` nor the possession family's raw add — so the dart
    /// leaves at the creator's flat 384 whatever the carpet is doing.
    /// Retail's slot 855 records exactly 384.
    ///
    /// The stamps, in retail's own order: the hand muzzle
    /// (`sub_68E50`), `word_0x26_38` = the token slot (the port spends
    /// that lane on the XP back-ref, as the possession arms do),
    /// impact 10/26, `subSpellIndex_0x2A_42` + `mana_0x90_144` +
    /// `byte_0x46_70` off the TOKEN, `position.z += caster fov` (which
    /// `muzzle_side` already carries), the `axis_0x9A_154x` aim point
    /// = the caster's position stepped 10240 along the launch bearing,
    /// the bearing itself, and `PrepareEventSound(…, -1, 9)`.
    fn mc2_cast_duel(&mut self, hx: Hx, m: usize, sub: Mc2SubSpell, p: PlayerPose) {
        let (mx, my, mz) = self.muzzle_side(p, self.mc2_fire_side(hx, m));
        let Some(i) = self.g.mc2_spawn_cast_proj(7, mx, my, mz) else {
            return; // pool full: no dart, NO cast sound
        };
        // The aim point: the CASTER's own position (not the muzzle)
        // stepped a flat 10240 along the launch bearing — the two
        // commented-out `*(x_DWORD *)v3 = *(x_DWORD *)(v8 + 76)` lines
        // above EF:57310 are that copy, dropped by the 2019 hand
        // conversion (see [[remc2-source-corruption-class]]); the
        // `MoveEntity_57FA0` call they feed survived intact.
        let mut dest = (p.x, p.y, p.z);
        Gen::polar_step(&mut dest, p.heading, p.pitch, 10240);
        let token_mana = self.g.ent[m].f140;
        let tier = self.g.ent[m].f71;
        {
            let e = &mut self.g.ent[i];
            e.id24 = self.g.human_tag[hx].0;
            e.f68 = 10;
            e.f69 = 26;
            e.f44 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
            e.f140 = token_mana;
            e.f71 = tier;
            e.f30 = p.heading;
            e.f32 = p.pitch;
            // `word_0x26_38` = the (15,14) token SLOT (see
            // [`no_token_slot_backref`]); was the bare index 14.
            e.f40 = if no_token_slot_backref() {
                14
            } else {
                m as u16
            };
            e.dest_x = dest.0;
            e.dest_y = dest.1;
            e.site_z = dest.2;
        }
        self.g.snd_player(hx, 9);
    }

    fn mc2_cast_metamorph(&mut self, hx: Hx, m: usize, sub: Mc2SubSpell, p: PlayerPose) {
        let model = sub.life.max(0) as u8;
        // sub_6A030 (EF:56325) mints at the caster's FULL 3-D
        // position — position_0x4C_76 forwarded untouched through
        // 4A190 to AddEventToMap; NO terrain clamp on the path
        // (mc2l0-sg t=7282: body born at carpet z 256, not ground 0).
        let z = p.z;
        let Some(s) = self.g.mc2_spawn_creature_model(model, p.x, p.y, z) else {
            return;
        };
        {
            let e = &mut self.g.ent[s];
            e.site_z = 12; // StageVar2 = 12 (metamorph pose-puppet)
            e.tick70 = model.wrapping_mul(8).wrapping_add(7); // action 8*M+7
            e.id24 = self.g.human_tag[hx].0; // caster's team → allied
            // ⭐ THE CTOR'S `dword_0x10_16 = slot % 100` SURVIVES —
            // `sub_6A030` makes no @0x10 store and neither does
            // `sub_1E4D0`. See
            // [`crate::engine::features::no_mc2_morph_keeps_ctor_10`].
            if crate::engine::features::no_mc2_morph_keeps_ctor_10() {
                e.f26 = 0; // the pre-dig scream-loop timer seed
            }
            // ⭐⭐⭐ `or BYTE PTR [ecx+0xc],0x1` at 0x8e928 — THE PUPPET
            // IS BORN HIDDEN. See [`no_metamorph_puppet_hidden`] for
            // the whole disassembled tail. The `&= 0xfe` correction
            // below it is the NON-LOCAL caster's arm; this is the
            // human column, which is the local player by
            // construction, so the bit stands.
            if !no_metamorph_puppet_hidden() {
                e.flags |= 1;
            }
        }
        self.g.ent[m].f146 = s as u16; // manifestation link (word_0x96_150)
        self.players[hx].metamorph = model; // hide the carpet, draw the creature
        // ⭐⭐⭐ THE METAMORPH CLOAK IS THE INVISIBILITY BIT.
        // `sub_6A030`'s first-tick block ends `v2x->byte[0] |= 0x21`
        // (EF:56335; shipped EXE 0x8E92F `or dl,0x21`) on the CASTER —
        // bit 0 is the draw-hide, bit 5 (0x20) is the SAME
        // scan-invisibility bit the Invisibility spell and the death
        // touchdown raise, and every mob scanner filters on it
        // (`sub_1BF90` :9155, `sub_1DBF0`'s wizard-watch tail :10294,
        // the archer's Scan A). The port hid the carpet from the
        // renderer only, so a metamorphed wizard stayed scannable.
        // mc2l0-spells-galore t=8104 slot 96 is the witness: a (5,1)
        // in the kind-2 hold (action 15, StageVar2 2) noticed the
        // metamorphed human 932 units away and broke to aggro
        // (StageVar2 10, action 14, speed → min_speed); retail leaves
        // it held. ⚠ ONE RETAIL FIELD, TWO PORT HOMES — the mirror
        // alone lives one frame (`tick_inner` rebuilds it from the
        // spell every tick), so the authoritative `player.invisible`
        // takes the write too. `invis_strength` is NOT set: retail
        // never touches `byte_0x1BF_447` here, so arming a spell does
        // not break a metamorph cloak.
        if !crate::mc2::roster::no_metamorph_cloak() {
            self.set_human_invisible(hx, true);
            self.g.player_invisible[hx] = true;
        }
        self.mc2_award_xp(self.g.human_tag[hx].0, 4, 1);
        self.g.snd_player(hx, 60);
    }

    /// The CASTER's live `actSpeed_0x82_130` as a cast thunk reads it
    /// mid-walk. See [`no_mc2_speed_token_live_actspeed`].
    pub(crate) fn mc2_caster_act_speed(&self, hx: Hx, p: PlayerPose) -> i16 {
        if no_mc2_speed_token_live_actspeed() {
            return p.speed;
        }
        self.pending_speed_base[hx].unwrap_or(p.speed)
    }

    /// Returns the spawned projectile's slot (None = pool full).
    fn mc2_launch(
        &mut self,
        hx: Hx,
        spell: usize,
        m: usize,
        arm: &DispatchArm,
        sub: Mc2SubSpell,
        p: PlayerPose,
    ) -> Option<usize> {
        // Hand muzzle — `sub_68E50` (EF:55595), called right after
        // every retail cast spawn: step the spawn point 256 units to
        // the firing hand's side (`caster.yaw ∓ 512`, pitch 0, so the
        // muzzle LIFT below is untouched), revert if that point sits
        // inside terrain, then copy it onto the projectile. The side
        // is the CASTER's own flag bits, which is why `hand_bits`
        // rather than a token register carries it (the
        // `dual_wield_muzzle` patch overrides it per token).
        let (mx, my, mz) = self.muzzle_side(p, self.mc2_fire_side(hx, m));
        // ⭐ THE ARMY AND ALLIANCE ARMS NEITHER LIFT NOR RE-PRICE.
        // Diffed statement by statement against their five siblings,
        // `sub_6C170` (EF:57659-81) and `sub_6CD20` (EF:58062-86) are
        // the only two direct `_4A190` arms with NEITHER the muzzle
        // lift NOR the mana copy: the `sub_693F0` fire block carries
        // `v6x->mana_0x90_144 = a1x->mana_0x90_144;` and
        // `v6x->position_0x4C_76.z += v1x->array_0x52_82.fov;`
        // (EF:55865-66), the mine `sub_6CAC0` carries both
        // (EF:57991-92) and so do both possession arms
        // (EF:56055-56 / EF:55970-71) — the army and the alliance
        // spawn at `&<caster>->position_0x4C_76`, run `sub_68E50`
        // (which only steps 256 LATERALLY, at pitch 0) and stop. So
        // they launch at the CASTER'S OWN z and keep the class-9
        // ctor's `mana_0x90_144 = 50`.
        // ⚠ Three of the four EF citations under the mana copy below
        // name the WRONG function: EF:57745 is `sub_6C3E0`, EF:57817
        // is `sub_6C620`, EF:58151 is `sub_6CFA0`; only the mine's
        // EF:57992 sits in the arm it claims. `sub_6C170` (the army,
        // EF:57638-57700) and `sub_6CD20` (the alliance,
        // EF:58039-58106) contain no `mana_0x90_144` write at all.
        // Measured on both Firefly Army casts in mc2l6-rsg: pair
        // 15630→15631 slot 6 — z retail **1145** (carpet 343's own z
        // that tick, per `explain`) vs port 1245 (+PLAYER_HH), mana
        // retail **50** vs port 33 (the token's purse); pair
        // 20561→20562 slot 102 — z 992/1092, mana 50/80.
        let bare = matches!(arm.subtype, 24 | 25);
        let mz = if bare { p.z } else { mz };
        // The caster token's own charge byte — retail's `a1x->byte_0x46_70`,
        // the tier index `mc2_spell_fire` reads to pick `sub`.
        let tier_index = self.g.ent[m].f71;
        // ⭐⭐⭐ THE CARPET BOOST IS THE **LIVE** `actSpeed_0x82_130`,
        // NOT THE TICK-TOP POSE — see
        // [`no_mc2_speed_token_live_actspeed`].
        let caster_speed = self.mc2_caster_act_speed(hx, p);
        let Some(i) = self.g.mc2_spawn_cast_proj(arm.subtype, mx, my, mz) else {
            return None; // pool full: no projectile, NO cast sound
            // (retail gates the sound on the spawn, EF:44224-39)
        };
        {
            let e = &mut self.g.ent[i];
            e.id24 = self.g.human_tag[hx].0;
            e.f68 = arm.impact.0;
            e.f69 = arm.impact.1;
            // The tier payload rides every projectile (EF:55864 —
            // the effect-state copy; carried damage / claim amount).
            //
            // ⭐⭐⭐ …EXCEPT ON THE THREE ARMS WHOSE THUNK HAS NO SUCH
            // STATEMENT AT ALL — AN ABSENCE IN AN ENUMERATED LIST IS
            // THE LAW. Scanning every human cast thunk for
            // `subSpellIndex_0x2A_42 =`: the band `sub_6DCA0` writes
            // `a4x->subSpellIndex_2` per arm (EF:44105/44128/44144/
            // 44158/44170/44183) and its fire block `sub_693F0`
            // OVERWRITES with the TOKEN's own @0x2A (EF:55864), the
            // alliance `sub_6CD20` (EF:58073) and Fool's Mana
            // `sub_6C870` (EF:57906) write the spell row's
            // `subSpellIndex_2`, the mine `sub_6CAC0` (EF:57993) the
            // tier index, the duel `sub_6B610` (EF:57300) the token's
            // — but the BASIC possession `sub_69900` (EF:56039-70),
            // the LEVELED possession block in `sub_69640`
            // (EF:55950-84) and the summon ARMY `sub_6C170`
            // (EF:57637-77) contain NO `subSpellIndex_0x2A_42` write
            // whatsoever. Their flyers therefore keep
            // `NewEvent_4A050`'s ctor default **100**
            // (Events.cpp:569, the port's `Gen::new_event`
            // `e.f44 = 100`).
            //
            // WITNESS (mc2l22, `MGC_RAW_SHADOW=1`, first 1200 pairs):
            // `(9,1) f2a` 121 rows across 76 slots, retail **100** vs
            // port 10 (e.g. t=9 slot 664; the dig's own head is t=495
            // slot 719), and `(9,17) f2a` 3 rows, retail **100** vs
            // port 20 (t=512 slot 718). The army arm is unwitnessed on
            // this take — mc2l6-rsg's two Firefly Army casts are the
            // place to confirm it.
            // ⭐ THE FOURTH MEMBER IS THE STEAL-MANA BOLT — the one
            // `sub_6DCA0` BAND arm with no `@0x2A` write (and whose
            // caller `sub_6B3E0` has none either). Full citation on
            // [`no_steal_2a_absence`].
            let absent_2a =
                matches!(arm.subtype, 1 | 17 | 24) || (arm.subtype == 8 && !no_steal_2a_absence());
            if !absent_2a || no_launch_2a_absence() {
                e.f44 = sub.sub_spell.clamp(0, u16::MAX as i32) as u16;
            }
            // ⭐ EXCEPT THE MAGIC MINE, THE ONE ARM THAT SHIPS THE TIER
            // INDEX INSTEAD OF THE PAYLOAD. `sub_6CAC0` (EF:57993) is
            // `v2x->subSpellIndex_0x2A_42 = a1x->byte_0x46_70;` where
            // every sibling writes
            // `SPELLS[spell].subspell[byte_0x46_70].subSpellIndex_2`
            // (fools' mana EF:57906, alliance EF:58073, and all six
            // `sub_6DCA0` band arms EF:44105-44170). It has to: the
            // (10,78) mine's own arm step re-INDEXES the spell row with
            // it (`SPELLS[23].subspell[subSpellIndex].subSpellIndex_2`
            // → maxLife, EF:29886-88) — see `mc2/effects.rs`
            // `mc2_mine_tick`.
            if arm.subtype == 29 && !crate::mc2::effects::no_mc2_mine() {
                e.f44 = tier_index as u16;
            }
            // ⭐ …EXCEPT THE MAGIC MINE AGAIN: `sub_6CAC0` has no
            // `byte_0x46_70` store on the carrier at all, only a READ
            // of the CASTER's. See [`no_mine_carrier_tier_byte`].
            if arm.charge && (arm.subtype != 29 || no_mine_carrier_tier_byte()) {
                e.f71 = sub.life.max(0) as u8;
            }
            // Launch = the carpet's facing; the projectile's z gets
            // the muzzle lift (pos.z += caster fov — the carpet
            // sits at pose z already).
            e.f30 = p.heading;
            e.f32 = p.pitch;
            // ⭐⭐⭐ AN ABSENCE IN AN ENUMERATED LIST — THE CLASS-9
            // FLYER IS BORN WITH `roll`/`fov` AT ZERO. The port's
            // `f34`/`f36` ARE retail's `roll_0x20_32`/`fov_0x22_34`
            // (`import_ent_mc2`: `f34: r.roll`), i.e. the DESIRED
            // yaw/pitch the servo chases — and NOT ONE of the twenty
            // class-15 fire handlers writes either word on the flyer
            // it spawns. Nor does the band spawner `sub_6DCA0`
            // (EF:44019-44238 contains no `roll_0x20_32`,
            // `fov_0x22_34`, `yaw_0x1C_28` or `pitch_0x1E_30` write at
            // all), and `NewEvent_4A050` memsets the record before
            // seeding its eleven named defaults (Events.cpp:565-578),
            // none of which is @0x20 or @0x22.
            // BYTE-VERIFIED: `sub_69900`'s complete store list in the
            // shipped `NETHERW.EXE` (file 0x8E100-0x8E240) is @0x82,
            // @0x43, @0x26, @0x44, @0x1a, @0x50, @0x90, @0x10, @0x9a
            // (the aim point), @0x1c and @0x1e — there is no
            // `mov %ax,0x20(%ebx)` and no `0x22(%ebx)` anywhere in it.
            // The port stamped the launch bearing into both, which the
            // NEXT tick's `mc2_flyer_tick` recomputes anyway (its
            // targetless arm is literally `e.f34 = e.f30`), so the
            // stamp only ever showed up as a BIRTH-TICK divergence in
            // the ungraded `roll` lane. `MGC_NO_MC2_LAUNCH_ROLL_ABSENCE=1`
            // restores it.
            if no_launch_roll_absence() {
                e.f34 = p.heading;
                e.f36 = p.pitch;
            }
            // ⭐⭐⭐ THE DIRECT ARMS TAKE THE CASTER BOOST **RAW** —
            // THE [384, 0x2000] CLAMP IS `sub_6DCA0`'s ALONE
            // (EF:44224-31, the band spawner's tail). Every
            // hand-written class-15 handler instead opens with a bare
            // `spawned->actSpeed_0x82_130 += caster->actSpeed_0x82_130`
            // and never clamps: possession basic (`sub_69900`
            // EF:56048) and leveled (`sub_69640` EF:55953), summon
            // army (`sub_6C170` EF:57662), magic mine (`sub_6CAC0`
            // EF:57984), alliance (`sub_6CD20` EF:58065). So a bolt
            // cast by a REVERSING wizard is born BELOW its own
            // `min_speed` and stays there — the class-9 mover never
            // floors it back up.
            //
            // ⭐ A LAW LANDED ON ONE CALL PATH IS NOT LANDED. The
            // RIVAL funnel has carried exactly this since mc2l6-rsg
            // t=1130 (`mc2_rival_cast_proj`, rivals.rs `let boosted =
            // if direct { … } else { … }`, same five citations), and
            // the human's possession pair got a one-off re-stamp
            // (`e.f126 = 384i32.saturating_add(p.speed …)` in
            // `mc2_possess_launch`, mc2l4 t=13 slot 303 = 336) — but
            // `mc2_launch` itself, which serves the army / mine /
            // alliance arms too, kept the clamp. mc2l6-rsg pair
            // 15630→15631: the human casts Firefly Army while
            // REVERSING at actSpeed −48 and retail records the (9,24)
            // at slot 6 with `speed` **336** = 384 + (−48); the port
            // floored the sum back to the ctor's own 384. (Both
            // Firefly Army casts show it — pair 20561→20562 slot 102
            // is the second.)
            //
            // ⚠ The `.max(0)` on the BAND leg is left exactly as the
            // rival twin leaves it: retail passes `a5 =
            // v1x->actSpeed_0x82_130` RAW (EF:55856 and the eleven
            // `sub_6DCA0` siblings) and every class-9 creator's base
            // speed is 384, so `clamp(384, ..)` swallows the
            // difference on every existing row. No witness, no change.
            let boosted = if matches!(arm.subtype, 1 | 17 | 24 | 25 | 29) {
                e.f126 as i32 + caster_speed as i32
            } else {
                (e.f126 as i32 + caster_speed.max(0) as i32).clamp(384, 0x2000)
            };
            e.f126 = boosted as i16;
        }
        // ⭐⭐⭐ THE LAUNCH AIM POINT (`axis_0x9A_154x`, our
        // `dest_x`/`dest_y`/`site_z`). EVERY human cast thunk but one
        // stamps the spawned flyer's @0x9A vector with the CASTER's
        // OWN position (not the muzzle, not the fov-lifted z) stepped
        // along the launch bearing:
        //   `sub_693F0`  band fire block  EF:55871-77   0x4000
        //   `sub_69640`  leveled possess  EF:55976-81   0x4000
        //   `sub_69900`  basic possess    EF:56059-64   **10240**
        //   `sub_6C170`  summon army      EF:57673-77   0x4000
        //   `sub_6CD20`  alliance         EF:58077-81   0x4000
        //   `sub_6B3E0`  steal mana       EF:57214-18   0x4000
        //   `sub_6CAC0`  magic mine       EF:57996-98   4096, pitch 0,
        //                                 then z = getTerrainAlt
        //   `sub_6B610`  duel dart        EF:57310      10240
        // — and only Fool's Mana (`sub_6C870`, EF:57867-57946) has no
        // such write. The port carried exactly ONE of the eight (the
        // duel dart, `mc2_cast_duel`); the other seven left the lane
        // at the ctor's zero. ⭐ A LAW LANDED ON ONE CALL PATH IS NOT
        // LANDED.
        //
        // The bearing is `wizext->nextEntity_0x18_24 + caster.yaw` /
        // `wizext->entityIndex2_0x1A_26 + caster.pitch`, i.e. the same
        // pair this helper already stamps as the launch yaw/pitch —
        // the port keeps no aim-offset lane, so `p.heading`/`p.pitch`
        // stand in, exactly as `mc2_cast_duel` does.
        //
        // WITNESS (mc2l22, `MGC_RAW_SHADOW=1`, first 1200 pairs):
        // `(9,1) dest_x/dest_y/dest_z` 121 rows EACH across 76 slots,
        // retail non-zero vs port 0 (t=9 slot 664 retail
        // 537/13129/684), and `(9,17)` 3 rows each (t=512 slot 718
        // retail 10107/33687/512). The dig's own head is mc2l22 t=495
        // slot 719, where retail records (61138, 26877, −183).
        //
        // ⚠ SCOPE: only the four DIRECT `_4A190` arms whose thunk was
        // read statement by statement in round 98 are landed here.
        // The BAND's own site (`sub_693F0` EF:55871-77, which would
        // serve every `mc2_dispatch_arm` subtype) and the mine's
        // terrain-alt variant are reported, not landed — the band
        // reaches this helper through paths (`sub_6A5C0`'s lightning
        // fan EF:56599-56680, EF:56141's 4096/pitch-0 site) that were
        // not audited.
        //
        // ⭐ ROUND 98 — THE SCOPE NOTE ABOVE IS NOW PAID OFF. The
        // whole class-15 fire table was enumerated site by site (see
        // [`mc2_launch_axis_reach`], which carries the twenty-row
        // table and the two shipped-EXE confirmations), so the band's
        // own `sub_693F0`/`sub_6A5C0` blocks and the two remaining
        // direct arms (spell 9 `sub_6AB00`, spell 13 `sub_6B3E0`)
        // are landed here too, keyed on the SPELL rather than on the
        // dispatch subtype. `MGC_NO_MC2_LAUNCH_AXIS_BAND=1` reverts
        // exactly that extension.
        // `MGC_NO_MC2_LAUNCH_AXIS_BAND=1` reverts ONLY the round-98
        // extension (spells 0/7/9/13) on the HUMAN column, leaving
        // 98-1's four direct arms and the rival funnel's own switch
        // independently attributable.
        let band_ext = matches!(spell, 0 | 7 | 9 | 13) && no_launch_axis_band();
        if let Some((reach, use_pitch, ground_snap)) =
            mc2_launch_axis_reach(spell, sub.life, arm.subtype)
        {
            if !no_launch_axis() && !band_ext {
                let mut dest = (p.x, p.y, p.z);
                Gen::polar_step(
                    &mut dest,
                    p.heading,
                    if use_pitch { p.pitch } else { 0 },
                    reach,
                );
                // The terrain-tail eight end
                // `axis.z = getTerrainAlt_10C40(&axis)`.
                if ground_snap {
                    dest.2 = self.g.ground_z(dest.0, dest.1) as i16;
                }
                let e = &mut self.g.ent[i];
                e.dest_x = dest.0;
                e.dest_y = dest.1;
                e.site_z = dest.2;
            }
        }
        // Every retail cast site copies the hand token's mana onto
        // the spawned projectile (`v6x->mana_0x90_144 =
        // a1x->mana_0x90_144` — the sub_693F0 fire block EF:55865
        // and each instant handler: army EF:57745, fools EF:57817,
        // mine EF:57992, alliance EF:58151). The lane is COMPARED
        // (mc2l0 t=2798 slot 172: retail 20 = the fireball hand's
        // purse; the class-9 ctor default 50 must not survive).
        if !bare {
            let token_mana = self.g.ent[m].f140;
            self.g.ent[i].f140 = token_mana;
        }
        // The fire block banks the caster's cast-charge meter into
        // the projectile's @0x10 scratch home and ZEROES the meter
        // (`v6x->dword_0x10_16 = wizext->byte_0x154; … = 0`,
        // EF:55869-70). The lightning T3 fan banks per spawn
        // (EF:56620) — the twins read meter/0 exactly like retail's
        // loop — and the possession arms overwrite the bank with
        // their own @0x10 law after this helper returns while
        // keeping the zero (EF:56058/55975).
        self.g.ent[i].f26 = self.wiz_charge[self.g.human_seat[hx].0 as usize] as i16;
        self.wiz_charge[self.g.human_seat[hx].0 as usize] = 0;
        // ⭐⭐⭐ `word_0x26_38` IS THE (15,x) TOKEN'S **POOL SLOT**,
        // NOT THE SPELL INDEX, AND TWO RETAIL READERS DEREFERENCE IT:
        // the impact XP award (`sub_6D8B0(id,
        // Entities[a1x->word_0x26_38]->model_0x40_64, 1)`, EF:62985)
        // — for which slot and index are interchangeable, because a
        // (15,N) token's model IS N — and `sub_68AC0`, the Magic
        // Mine's swallow, which also writes the token's tier back
        // into the mine and RE-ARMS the token's window
        // (`v5x->word_0x2E_46 = 1`, EF:55441-44). The second reader
        // is unreachable from an index, so the lane carries the slot
        // and [`Gen::mc2_token_model`] resolves it back.
        if !no_token_slot_backref() {
            self.g.ent[i].f40 = m as u16;
        }
        // The local player's FIREBALL swaps to the star-shaped
        // muzzle/aim sprite 42 (`SetEntityIndex_49C90(v17x, 42)`,
        // gated local-player && spell 0 — EF:30291): index + frame
        // only, the row-340 extent quad stays (49C90, not 49CD0).
        if spell == 0 {
            let e = &mut self.g.ent[i];
            e.type86 = 42;
            e.frame88 = 0;
        }
        Some(i)
    }

    /// The crosshair instrument's MC2 arm (P-class; `aim_preview`
    /// routes MC2-bound hands here): the target the hand's spell's
    /// PROJECTILE would acquire on its FIRST flight tick if launched
    /// this instant — the pure [`Gen::mc2_aim_scan`] twin under the
    /// launch pose. Retail MC2 draws NO reticle (the aim feedback IS
    /// the sprite-42 projectile curving, docs/traces/mc2-autoaim.md
    /// §4/mc2-mouse-aim.md §4); this is an opt-in predictor
    /// (deliberate), not a faithful surface. None = non-acquiring
    /// spell or empty cone.
    pub(crate) fn mc2_aim_preview(
        &self,
        hx: Hx,
        p: PlayerPose,
        right: bool,
        spell: usize,
    ) -> Option<AimLock> {
        // The would-be projectile subtype: the sub_6DCA0 band + the
        // direct class-9 arms (possess/summon/mine/alliance).
        let tier = self.mc2_book[hx].sel.get(spell).copied().unwrap_or(0) as usize;
        let life = self
            .g
            .assets
            .spells
            .get(spell)
            .map_or(0, |r| r.tiers[tier.min(2)].life);
        let subtype = Self::mc2_dispatch_arm(spell, life)
            .map(|a| a.subtype)
            .or(match spell {
                // Possession picks its ENTITY off the tier's life:
                // 0 → the basic (9,1), 1..3 → the leveled (9,17)
                // (EF:55946-49). Both share the model-1/0x11 aim list.
                1 if life == 0 => Some(1),
                1 => Some(17),
                0xE => Some(7), // the duel dart (`sub_6B610` EF:57291)
                0x13 => Some(24),
                0x17 => Some(29),
                0x18 => Some(25),
                _ => None,
            })?;
        let (_, _, speed, max_life, _, _) = *CREATORS.iter().find(|c| c.0 == subtype)?;
        let (mx, my, mz) = self.muzzle(p, right);
        let probe = super::proj::AimProbe {
            x: mx,
            y: my,
            z: mz,
            yaw: p.heading,
            pitch: p.pitch,
            model: subtype,
            own: self.g.human_tag[hx].0,
            // The would-be shot's owner is the human carpet, a wizard
            // row like any other (`sub_67CB0`'s owner hoist).
            range: self.g.mc2_owner_lock_range(self.g.human_tag[hx].0),
            reach: speed as i64 * max_life as i64,
        };
        let slot = self.g.mc2_aim_scan(&probe, &[])?;
        let e = &self.g.ent[slot as usize];
        Some(AimLock {
            x: e.x as f32 / 256.0,
            z: e.y as f32 / 256.0,
            // The acquire aims at the +78 half-height point
            // (`sub_655C0`) — castles at the raw z (the flag).
            alt: e.aim_z() as f32 / 256.0,
        })
    }

    // ---- level-init defaults / death scatter -------------------------------

    /// The MC2 level-start book: FIREBALL (0) and POSSESS (1) at 0 XP
    /// (MC1 by contrast inits spell-less). The adopt order binds
    /// fireball → left, possess → right via the pickup's own v12
    /// quick-slot law.
    ///
    /// CORRECTION — do NOT re-derive from the name this once cited:
    /// `SetDefaultSpells_5C0A0` (`Spells.cpp:110`) grants NOTHING. It
    /// only rewrites the static SPELLS table's `isEnabled_1` /
    /// `fontType_0x1B` / `maxManaLimit_A` flags. The real grant is
    /// `InitialiseSpells_54A50`'s gate (`EventsFunctions.cpp:38721-62`):
    /// walk indices 0..25 ascending, enable the human's entitled set —
    /// the level's authored `starting_spells` row on campaign level 0
    /// or a direct `--level N` launch, the CARRIED book thereafter,
    /// always minus `blocked_spells` — then first enabled → left,
    /// second → right at tier 0.
    ///
    /// Spells are HOARDED across levels in both games — neither engine
    /// re-grants a book each level. Retail's fallback to the authored
    /// row applies ONLY when there is no carry (campaign level 0, or a
    /// direct `--level N`); on campaign levels > 0 the carried book is
    /// the whole story, and the level's `allowed`/`blocked` rows only
    /// ever TAKE spells away (MC1 does this from index 025). MC2's
    /// campaign does it too: the human's `BlockedSpells` row withholds
    /// Alliance on maps 16/17 and the four earth spells on the finale
    /// (map 24), keeping them known with their XP
    /// ([`Self::mc2_bank_withheld`]).
    ///
    /// So seeding `{0, 1}` unconditionally here is a floor retail does
    /// not have: a spell permanently lost to the wraith steal would be
    /// handed back by us and not by retail. Correct for level 000
    /// (whose row IS `{0,1}`) and harmless for the hands, but OPEN —
    /// see docs/ROADMAP.md. The campaign carry itself is already right
    /// (`apply_campaign_book`).
    pub(crate) fn mc2_seed_default_spells(&mut self, hx: Hx) {
        for s in [0usize, 1] {
            if self.mc2_book[hx].ent[s] != 0 {
                continue;
            }
            let (px, py, pz) = self.human_pose(hx);
            if let Some(m) = self.mc2_new_spell_token(s as u8, px, py, pz) {
                self.mc2_adopt_manifestation(hx, m, s);
                self.g.mc2_spell_tokens[hx].0 |= 1 << s;
            }
        }
        self.mc2_rebind_hands_canonical(hx);
    }

    /// The level-start book beyond the `{0, 1}` floor: mint a token for
    /// every listed spell the book does not hold yet, in canonical
    /// order, exactly like [`Self::mc2_seed_default_spells`] — and at
    /// the same point of the load (call it BEFORE `set_mc2_wizards`),
    /// so the tokens take the slots retail's `sub_5C950` gives the
    /// human's carried book, ahead of the rivals. The conformance
    /// instruments (`terrain-check`) read the list off a take's record
    /// 0; the app's campaign carry keeps `mc2_grant_plausible`.
    pub fn mc2_grant_start_book(&mut self, hx: Hx, spells: &[u8]) {
        if !matches!(self.game(), crate::ids::GameId::Mc2) {
            return;
        }
        for &sp in spells {
            let s = sp as usize;
            if s >= 26 || self.mc2_book[hx].ent[s] != 0 {
                continue;
            }
            let (px, py, pz) = self.human_pose(hx);
            if let Some(m) = self.mc2_new_spell_token(s as u8, px, py, pz) {
                self.mc2_adopt_manifestation(hx, m, s);
                self.g.mc2_spell_tokens[hx].0 |= 1 << s;
            }
        }
        self.mc2_rebind_hands_canonical(hx);
    }

    /// The MC2 level-init hand assignment (`InitialiseSpells_54A50`,
    /// EF:38664-38762): clear both hands, then walk the spell indices
    /// in canonical order — `spellIndex_D94FF` (GameUI.cpp:59) is the
    /// IDENTITY over 0..25 — binding the first enabled spell to the
    /// LEFT hand and the second to the RIGHT. Fewer than two enabled
    /// leaves the remaining hand at -1, and the cast path suppresses
    /// that button. Tier stays 0 (`SubSpellIndex*` derive from the
    /// per-level-zeroed `array_0x437`, EF:38659 / :59421).
    ///
    /// A level start must NOT reuse the jar-pickup law in
    /// [`Self::mc2_adopt_manifestation`]: that binds left, then right,
    /// then OVERWRITES left for every further spell, so a batch of N
    /// grants ends with left = the LAST granted and right = the
    /// SECOND. mc2:003's `{0,1,2,3,4,6,11,12}` came out Beyond Sight
    /// (12) / Possession (1) instead of Fireball / Possession. The
    /// pickup law is right for actual pickups and is unchanged.
    pub(crate) fn mc2_rebind_hands_canonical(&mut self, hx: Hx) {
        self.mc2_set_hand(hx, false, -1);
        self.mc2_set_hand(hx, true, -1);
        for s in 0..26usize {
            if self.mc2_book[hx].ent[s] == 0 {
                continue;
            }
            if self.mc2_book[hx].left == -1 {
                self.mc2_set_hand(hx, false, s as i8);
            } else if self.mc2_book[hx].right == -1 {
                self.mc2_set_hand(hx, true, s as i8);
                break;
            }
        }
    }

    /// Wizard-death token scatter (`sub_5E310` EF:60137-62): every
    /// owned manifestation becomes a collectible jar again — state
    /// 3M+1, the in-book bit cleared, scattered ±256 around the
    /// CORPSE, life `rand%90 + 200`.
    ///
    /// The book entry becomes a BOOLEAN 1, not 0 (EF:60146): that
    /// marker is the whole memory of what the wizard knew, and
    /// `sub_5CF40` re-mints exactly the entries that are non-zero.
    /// Zeroing it here (as this arm did while it was unwired) would
    /// have made every death a permanent spellbook wipe.
    ///
    /// The HANDS are untouched — `SpellIndexLeft/Right` are outside
    /// this loop and survive death (mc2l3 keeps 0/1 across both).
    ///
    /// DEVIATION: retail rolls the three draws per token off the dying
    /// WIZARD's private LCG (`a1x->rand_0x14_20`), which this port has
    /// no home for — the human owns no pool record, so its private
    /// stream is outside the sim. A COPY of the token's own seed
    /// stands in: same constants, same shape, different offsets. Two
    /// things it deliberately is NOT — the world stream (which would
    /// desync every other entity's draws on the landing tick) and the
    /// token's live `rand` field (retail's scatter never writes it;
    /// mc2l3 t=15300 keeps all 26 seeds at their allocation values).
    /// OPEN: import `carpet.rand` and roll the real stream.
    pub(crate) fn mc2_scatter_spells(&mut self, hx: Hx, p: PlayerPose) {
        // ⭐⭐ ONE RUNNING STREAM, THE DYING CARPET'S OWN (`sub_5E310`
        // EF:60153-61): three draws per scattered token, CONTINUING
        // across all 26 — not each token's private seed restarted 26
        // times, which is what the port did while the human had no
        // pool record to keep a stream in. `apply_player_damage` now
        // advances that stream once per intake and the importer seeds
        // it, so the value standing here is the one retail scatters
        // with. mc2l0 t=11192: twelve draws, three each for the four
        // owned spells.
        let cs = self.mc2_carpet_slot[hx] as usize;
        let mut carpet_rand = if cs != 0 && cs < self.g.ent.len() {
            self.g.ent[cs].rand
        } else {
            0
        };
        // Patch option `no_spell_loss` (docs/DEVIATIONS.md): the
        // scatter is COSMETIC — a fresh decaying (15,M) pickup jar per
        // owned spell on the same draws, while the book keeps naming
        // its live tokens (so `mc2_player_respawn` re-mints nothing).
        // The token tick's book gate refuses to re-collect a spell
        // the wizard holds, so the cosmetic jar just expires.
        let keep = self.patches.no_spell_loss && !self.strict_retail;
        for spell in 0..26usize {
            let m = self.mc2_book[hx].ent[spell] as usize;
            if m == 0 {
                continue;
            }
            if !keep {
                self.mc2_book[hx].ent[spell] = 1; // the boolean "still known" marker
            }
            // ⚠ SIXTEEN-BIT STEPS. `rand_0x14_20` is a `uint16_t`
            // (global_types.h:331), so every store truncates — and
            // while `& 0x1FF` cannot tell the widths apart, `% 0x5A`
            // very much can (90 is not a power of two, so it reads the
            // whole word). Running the shared 32-bit helper here put
            // the scatter's x/y exactly right and its life lane
            // exactly wrong.
            let mut draw = || {
                carpet_rand = carpet_rand.wrapping_mul(9377).wrapping_add(9439) & 0xFFFF;
                carpet_rand
            };
            let r1 = draw();
            let r2 = draw();
            let x = p.x.wrapping_add((r1 & 0x1FF) as u16).wrapping_sub(256);
            let y = p.y.wrapping_add((r2 & 0x1FF) as u16).wrapping_sub(256);
            let life = (draw() % 0x5A + 200) as i32;
            if keep {
                if let Some(j) = self.mc2_new_spell_token(spell as u8, x, y, p.z) {
                    let e = &mut self.g.ent[j];
                    e.tick70 = (spell as u8).wrapping_mul(3).wrapping_add(1);
                    e.act_life = life;
                    e.f26 = 0;
                }
                // No window crosses the death. The strict arm below
                // zeroes the scattered token's `word_0x2E_46`, so no
                // effect body ever runs again; the KEPT token must end
                // its window the same way — through the expiry the
                // countdown would have reached — or a live speed
                // window keeps slamming the flight columns on the
                // corpse and on the respawned wizard alike. The castle
                // column (2) counts something else and is left alone.
                // See [`World::player_death_clear_effects`].
                if spell != 2 && self.g.ent[m].f26 > 0 {
                    self.g.ent[m].f26 = 0;
                    self.mc2_cast_expire(hx, spell, m);
                }
                continue;
            }
            {
                let e = &mut self.g.ent[m];
                e.tick70 = (spell as u8).wrapping_mul(3).wrapping_add(1);
                e.act_life = life;
                // ⭐⭐ THE SCATTER DOES **NOT** STOP THE WINDOW
                // COUNTER — `word_0x2E_46` IS LEFT STANDING. This
                // `e.f26 = 0` was an invention; see
                // [`no_mc2_scatter_keeps_window`]. The effect body is
                // already unreachable without it: `mc2_cast_tick`
                // requires the BOOK entry (now the boolean 1) and
                // `tick70 == 3M`, and this loop has just moved the
                // record to 3M+1.
                if no_mc2_scatter_keeps_window() {
                    e.f26 = 0;
                }
                e.flags &= !1;
            }
            self.g.move_relink(m, x, y, p.z);
        }
        if cs != 0 && cs < self.g.ent.len() {
            self.g.ent[cs].rand = carpet_rand;
        }
        if !keep {
            self.g.mc2_spell_tokens[hx].0 = 0;
        }
    }
}

// ------------------------------------------------------------ snapshot

use crate::snapshot::{Reader, Snap, SnapshotError, Writer};

impl Snap for Mc2Spellbook {
    fn put(&self, w: &mut Writer) {
        let Mc2Spellbook {
            ent,
            xp_vol,
            xp_bank,
            levels,
            sel,
            left,
            right,
            ring,
        } = self;
        w.put(ent);
        w.put(xp_vol);
        w.put(xp_bank);
        w.put(levels);
        w.put(sel);
        w.put(left);
        w.put(right);
        w.put(ring);
    }
    fn get(r: &mut Reader) -> Result<Self, SnapshotError> {
        Ok(Mc2Spellbook {
            ent: r.get()?,
            xp_vol: r.get()?,
            xp_bank: r.get()?,
            levels: r.get()?,
            sel: r.get()?,
            left: r.get()?,
            right: r.get()?,
            ring: r.get()?,
        })
    }
}

#[cfg(test)]
mod posses_axis_reach_tests {
    //! ⭐⭐⭐ THE POSSESSION BOLT'S AIM REACH IS THE SPAWNER'S, NOT THE
    //! TIER ROW'S (round 148, dig w148o; see
    //! [`super::no_mc2_posses_axis_reach_by_subtype`] for the citation,
    //! the shipped bytes and the mc2l11 arithmetic).
    //!
    //! ⛔ UNIT TEST, NOT A FIXTURE: the lane is the raw shadow's
    //! `(9,1) dest_x/dest_y/dest_z`, ungraded by construction, and
    //! replay re-imports every pool record at the anchor.
    use super::mc2_launch_axis_reach;

    /// The held re-fire's shape: `subtype = 1` (the basic `(9,1)` bolt
    /// `sub_69900` mints) carrying the HELD TIER's subspell row, whose
    /// `life_0x1A` is 1 or 2. Both must answer `sub_69900`'s
    /// `push $0x2800` = 10240.
    ///
    /// POSITIVE CONTROL: the leveled `(9,17)` arm, which keeps
    /// `sub_69640`'s 0x4000 on the same rows, and the fireball, which
    /// this law does not touch at all. REVERSION PROOF: fails with
    /// `MGC_NO_MC2_POSSES_AXIS_REACH_BY_SUBTYPE=1`, where the two
    /// held-tier rows answer 0x4000.
    #[test]
    fn the_possession_reach_follows_the_spawned_subtype() {
        let reach = |life: i8, subtype: u8| mc2_launch_axis_reach(1, life, subtype).unwrap().0;

        // POSITIVE CONTROL: an un-held tier-0 cast is unchanged, and
        // so is every leveled bolt.
        assert_eq!(reach(0, 1), 10240, "tier 0 mints (9,1): sub_69900");
        assert_eq!(reach(1, 17), 0x4000, "tier 1 mints (9,17): sub_69640");
        assert_eq!(reach(2, 17), 0x4000, "tier 2 mints (9,17): sub_69640");
        // POSITIVE CONTROL: a spell this law does not touch.
        assert_eq!(
            mc2_launch_axis_reach(0, 0, 0).unwrap().0,
            0x4000,
            "the fireball band keeps sub_693F0's 0x4000"
        );

        // THE LAW: the held re-fire — `sub_68DE0` calls `sub_69900`
        // directly while `byte_0x46_70` still names tier 1 or 2.
        assert_eq!(reach(1, 1), 10240, "held tier 1 re-fires a BASIC bolt");
        assert_eq!(reach(2, 1), 10240, "held tier 2 re-fires a BASIC bolt");
    }
}

#[cfg(test)]
mod fools_retaliation_arm_tests {
    //! The two ABSENCES in `sub_36770`/`sub_36850`'s enumerated store
    //! lists, both disassembled from the shipped `NETHERW.EXE` (see
    //! [`super::no_fools_splash_arm`] / [`super::no_fools_lightning_id`]).
    //! Neither has an exemplar in the recording corpus — no take
    //! splashes a fool's retaliation, and no tier-2/3 Fool's Mana is
    //! ever claimed — so these are the lane, per docs/CONFORMANCE.md.
    use crate::engine::features::{FeatureAssets, Planes};
    use crate::engine::world::World;
    use crate::ids::GameId;
    use crate::mc1::mobs::MobCtx;
    use crate::mc2::spells::MC2_SPELL_ROWS;

    /// A flat MC2 world; `tile_type` 0 makes every tile WATER
    /// (`Gen::cap_bit` -> 1), which is `sub_36770`'s splash gate.
    fn world(water: bool) -> World {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![if water { 0 } else { 5 }; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let assets = FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: Vec::new(),
            build_dat: Vec::new(),
            bldgprm: Vec::new(),
            spells: vec![Default::default(); MC2_SPELL_ROWS],
            mc2_sprite_ext: Vec::new(),
            mc1_sprite_ext: Vec::new(),
        };
        World::new_for_game(planes, &[], 1, assets, GameId::Mc2)
    }

    fn ctx(g: &mut crate::engine::features::Gen) -> MobCtx {
        *g.view_mut(crate::engine::world::Hx::FIRST) = crate::engine::features::SeatView {
            x: 200 << 8,
            y: 200 << 8,
            z: 100,
            ..Default::default()
        };
        MobCtx {
            phx: crate::engine::world::Hx::FIRST,
            strict: false,
            patches: crate::patches::WorldPatches::RETAIL,
            mc2_turn: 0,
        }
    }

    /// The sphere, with a PARENT that is not its own slot — the whole
    /// point of the fused-`id24` hazard (`id24` is the port's home for
    /// BOTH retail's `@0x1A` and `@0x28`, and on a (10,57) the
    /// importer resolves it to `parentId_0x28_40`).
    fn sphere(w: &mut World, parent: u16) -> usize {
        let s =
            w.g.mc2_spawn_mana_sphere(57, 100 << 8, 100 << 8, 100)
                .expect("the fool's sphere spawned");
        w.g.ent[s].id24 = parent;
        w.g.ent[s].f44 = 500;
        assert_ne!(
            parent as usize, s,
            "the parent is not the sphere's own slot"
        );
        s
    }

    #[test]
    fn the_fools_water_splash_owns_itself() {
        // `sub_36770`'s splash arm is NETHERW.EXE 0x5B014-0x5B045 in
        // full: push 5 / push 10 / push &sphere->position / _4A190 /
        // test / je / <pool index> / push 27 / push -1 / push idx /
        // PrepareEventSound. NOT ONE STORE on the new record, so it
        // keeps NewEvent_4A050's `id_0x1A_26 = <own pool index>`.
        let mut w = world(true);
        let s = sphere(&mut w, 7);
        let c = ctx(&mut w.g);
        w.g.mc2_fools_bolt(s, 0, (10, 0), 4242, &c);
        let splash = (1..w.g.ent.len())
            .find(|&j| w.g.ent[j].class64 == 10 && w.g.ent[j].model65 == 5)
            .expect("the water tile minted the (10,5) splash");
        assert_eq!(
            w.g.ent[splash].id24, splash as u16,
            "the splash takes NO id_0x1A_26 (NETHERW.EXE 0x5B014-0x5B045 \
             has no store at all), so it keeps its own-slot ctor seed"
        );
    }

    #[test]
    fn the_dry_retaliation_mints_no_splash() {
        // The control: the arm is gated on the WATER tile
        // (`sub_104D0_terrain_tile_is_water`, 0x5B003-0x5B012).
        let mut w = world(false);
        let s = sphere(&mut w, 7);
        let c = ctx(&mut w.g);
        w.g.mc2_fools_bolt(s, 0, (10, 0), 4242, &c);
        assert!(
            !(1..w.g.ent.len()).any(|j| w.g.ent[j].class64 == 10 && w.g.ent[j].model65 == 5),
            "no splash off a dry tile"
        );
    }

    #[test]
    fn the_thunder_retaliation_carries_the_spheres_own_slot() {
        // `sub_36850`: 0x5B093 `mov 0x1a(%eax),%ax` / 0x5B097
        // `mov %ax,0x1a(%ebx)` — %eax is the SPHERE, and a (10,57)'s
        // `@0x1A` is its own pool index (`sub_50130`, file 0x74930,
        // never writes @0x1A, so NewEvent's seed stands). NOT its
        // `parentId_0x28_40`, which is what the port's fused `id24`
        // handed it.
        let mut w = world(false);
        let s = sphere(&mut w, 7);
        let c = ctx(&mut w.g);
        w.g.mc2_fools_bolt(s, 9, (10, 23), 4242, &c);
        let bolt = (1..w.g.ent.len())
            .find(|&j| w.g.ent[j].class64 == 9 && w.g.ent[j].model65 == 9)
            .expect("the tier-2 arm minted its thunder bolt");
        assert_eq!(
            w.g.ent[bolt].id24, s as u16,
            "the thunder bolt copies the SPHERE's @0x1A (its own slot), \
             not the Fool's-Mana caster"
        );
    }
}

#[cfg(test)]
mod shield_billing_tests {
    use crate::engine::features::{FeatureAssets, Planes};
    use crate::engine::world::Hx;
    use crate::engine::world::{PlayerCommand, PlayerPose, World};
    use crate::ids::GameId;
    use crate::mc1::mobs::PLAYER_TARGET;
    use crate::mc2::spells::{MC2_SPELL_ROWS, Mc2SpellRow};

    fn away() -> PlayerPose {
        PlayerPose::from_tiles(10.0, 105.0 / 8.0, 10.0, 0.0, 0.0, 0.0)
    }

    /// A flat MC2 world holding SHIELD (6) at tier 0, with the spell
    /// row synthesized at retail's own Shield numbers except for
    /// `life_0x1A`, which is the arm selector under test, and
    /// `maxManaLimit_A`, pinned to 0 so `sub_68D50`'s castle-pool
    /// upkeep leg is out of the picture (retail's Shield III wants a
    /// 60,000-mana castle; the law has nothing to do with that leg).
    /// Returns the world and the manifestation's pool slot.
    fn shield_world(life: i8) -> (World, usize) {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let mut spells = vec![Mc2SpellRow::default(); MC2_SPELL_ROWS];
        spells[6].byte_0 = 3;
        for t in 0..3 {
            // SPELLS.DAT spell 6 tier 2: manaCost 10000, word_0x18 301.
            spells[6].tiers[t].mana_cost = 10_000;
            spells[6].tiers[t].word_0x18 = 301;
            spells[6].tiers[t].max_mana_limit = 0;
            spells[6].tiers[t].life = life;
        }
        let assets = FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: Vec::new(),
            build_dat: Vec::new(),
            bldgprm: Vec::new(),
            spells,
            mc2_sprite_ext: Vec::new(),
            mc1_sprite_ext: Vec::new(),
        };
        let mut w = World::new_for_game(planes, &[], 1, assets, GameId::Mc2);
        w.mc2_grant_plausible(Hx::FIRST, &[(6, 0)]);
        let m = w.mc2_book[Hx::FIRST].ent[6] as usize;
        assert!(m != 0, "shield was granted");
        w.mc2_book[Hx::FIRST].left = 6;
        // Fund the purse the supported way (a CLAIMED sphere lifts the
        // ceiling the census re-derives every lap; a bare
        // `player.mana =` is clamped back).
        let (px, py, _) = w.human_pose(Hx::FIRST);
        let b = w.g.spawn_mana_ball(px, py, 4000).expect("purse");
        w.g.ent[b].f140 = 200_000;
        w.g.ent[b].f144 = PLAYER_TARGET;
        // The adopt path stamps a 64-tick cooldown (`word_0x36_54`);
        // run it off and let the census lift the ceiling.
        for _ in 0..70 {
            w.tick(away(), PlayerCommand::default());
        }
        w.set_human_mana(Hx::FIRST, w.human_mana_max(Hx::FIRST));
        assert!(
            w.human_mana(Hx::FIRST) >= 10_000,
            "the rig must be able to afford the 10,000 cast; got {}",
            w.human_mana(Hx::FIRST)
        );
        assert_eq!(w.g.ent[m].f26, 0, "the window is not armed yet");
        assert_eq!(w.g.ent[m].f54, 0, "the grant cooldown has run off");
        (w, m)
    }

    fn fire(w: &mut World) {
        w.tick(
            away(),
            PlayerCommand {
                fire_left: true,
                ..Default::default()
            },
        );
    }

    /// THE ARM TICK. Retail's `life_0x1A == 0` arm calls `sub_68DE0`
    /// with `word_0x2E_46 == word_0x30_48`, so it parks `-maxMana` in
    /// the caster's regen lane. The `life_0x1A == 1` arm has already
    /// decremented, so the same call takes the else and merely PINS
    /// the regen — Shield III's 10,000 is never charged.
    ///
    /// Witness: mc2l6-rival-spells-galore t=22531 (`f2e` 301 → 300,
    /// the carpet's `flags` 269 → 4194573 = `byte[2] |= 0x40`), where
    /// retail's `d88` goes 739 → 0 and `mana` stays 1475227 for the
    /// whole window; the port stamped −10000 and landed on 1465227.
    #[test]
    fn shield_iii_arms_without_paying_where_shield_i_pays_in_full() {
        // ── the CONTRAST arm: `life_0x1A == 0` bills ──────────────
        let (mut w, m) = shield_world(0);
        fire(&mut w);
        assert!(w.g.ent[m].f26 > 0, "the shield window armed (life 0)");
        assert_eq!(
            w.debug_player_mana_delta(Hx::FIRST),
            -10_000,
            "sub_6A480's `life_0x1A == 0` arm calls sub_68DE0 BEFORE \
             the decrement, so the first-tick full-cost debit lands \
             (EF:56512-14, NETHERW.EXE 0x8ecf8 call / 0x8ecfd dec)"
        );

        // ── the LAW arm: `life_0x1A == 1` never reaches the debit ──
        let (mut w, m) = shield_world(1);
        fire(&mut w);
        assert!(w.g.ent[m].f26 > 0, "the shield window armed (life 1)");
        assert_eq!(
            w.debug_player_mana_delta(Hx::FIRST),
            0,
            "sub_6A480's `life_0x1A == 1` arm decrements FIRST \
             (NETHERW.EXE 0x8ed29..0x8ed30 dec / 0x8ed34 call), so \
             sub_68DE0's `v2 == word_0x30_48` test is false by \
             construction and it can only PIN the regen"
        );
    }

    /// THE LAST TICK. Because the tier-2 arm hands `sub_68DE0` the
    /// POST-decrement counter, the tick the window ends on passes
    /// `v2 == 0` — and `if (v2 && manaRegen > 0)` is then false, so
    /// the wizard's freshly recomputed regen SURVIVES its own last
    /// shield tick.
    ///
    /// Witness: rsg t=22721, the re-press cancel (`move_bits` 0 → 16;
    /// `sub_5F660` case 0xE pins `f2e` to 1, `f2e` 111 → 0). Retail's
    /// `d88` goes 0 → 739 and `mana` 1475165 → 1475904 at t=22722;
    /// the port pinned and held flat — the recorded −739 wall.
    #[test]
    fn shield_iii_does_not_pin_the_regen_on_the_tick_its_window_ends() {
        let (mut w, m) = shield_world(1);
        fire(&mut w);
        // A mid-window tick DOES pin (`v2 != 0`) — the control that
        // stops this test passing on a port that never pins at all.
        w.tick(away(), PlayerCommand::default());
        assert!(w.g.ent[m].f26 > 1, "still mid-window");
        assert_eq!(
            w.debug_player_mana_delta(Hx::FIRST),
            0,
            "a mid-burst shield tick pins the regen (sub_68DE0's else)"
        );
        // Now the counter retail's re-press cancel manufactures.
        w.g.ent[m].f26 = 1;
        w.tick(away(), PlayerCommand::default());
        assert_eq!(w.g.ent[m].f26, 0, "the window ended on that tick");
        assert!(
            w.debug_player_mana_delta(Hx::FIRST) > 0,
            "the post-decrement counter is 0, so sub_68DE0 pins \
             NOTHING and the wizard's own recompute stands; got {}",
            w.debug_player_mana_delta(Hx::FIRST)
        );
    }

    /// ⭐⭐⭐ **SHIELD XP IS PAID ON THE ABSORB, NOT ON THE CAST.**
    /// `sub_6A480` never calls `sub_6D8B0`; the only spell-6 award
    /// site in the shipped image is `sub_5EFA0`'s absorb gate
    /// (file 0x8399D `push $0x6`, ABOVE the CHARGED/ARMED split).
    /// See [`super::no_mc2_shield_xp_on_absorb`].
    ///
    /// ⛔ UNGRADED: `xp_vol` is a WIZEXT lane and the book is
    /// re-imported at every anchor, so a recording fixture cannot
    /// hold this. A unit test is the lane.
    ///
    /// POSITIVE CONTROL: the same letter arriving with NO shield
    /// window open credits NOTHING — so this cannot pass on a port
    /// that simply credits spell 6 on every hit, and the +1 below
    /// cannot pass on a port that never credits spell 6 at all.
    #[test]
    fn shield_xp_is_paid_on_the_absorb_and_not_at_the_cast() {
        // ── POSITIVE CONTROL: a hit with no shield window ─────────
        let (mut w, _) = shield_world(0);
        w.players[Hx::FIRST].grace = 0; // the spawn-grace memset wipes every channel
        w.g.pmail_mut(Hx::FIRST)[0] = (400, 1);
        w.tick(away(), PlayerCommand::default());
        assert_eq!(
            w.mc2_book[Hx::FIRST].xp_vol[6],
            0,
            "sub_5EFA0's award is INSIDE `byte[1] & 0x40 || byte[2] & 0x40` \
             — an unshielded letter pays no shield XP"
        );

        // ── the law ───────────────────────────────────────────────
        let (mut w, m) = shield_world(0);
        fire(&mut w);
        assert!(w.g.ent[m].f26 > 0, "the shield window armed");
        assert!(
            w.human_shield(Hx::FIRST),
            "and the CHARGED stage is stamped"
        );
        assert_eq!(
            w.mc2_book[Hx::FIRST].xp_vol[6],
            0,
            "sub_6A480 has NO sub_6D8B0 call — the cast pays no XP \
             (the 33 e8-rel32 callers of 0x920B0 place none in \
             0x8F200..0x8F560)"
        );
        // Tens of ticks of armed window, still unhit, still unpaid.
        for _ in 0..8 {
            w.tick(away(), PlayerCommand::default());
        }
        assert_eq!(
            w.mc2_book[Hx::FIRST].xp_vol[6],
            0,
            "a shield window that absorbs nothing earns nothing"
        );

        // Now the letter.
        w.players[Hx::FIRST].grace = 0;
        let life0 = w.human_life(Hx::FIRST);
        w.g.pmail_mut(Hx::FIRST)[0] = (400, 1);
        w.tick(away(), PlayerCommand::default());
        assert_eq!(
            w.human_life(Hx::FIRST),
            life0 - 100,
            "the CHARGED stage quartered the letter, so it really was absorbed"
        );
        assert_eq!(
            w.mc2_book[Hx::FIRST].xp_vol[6],
            1,
            "sub_5EFA0 file 0x8399D: push $0x1 / push $0x6 / push the \
             victim's slot / call 0x920B0, above the absorb split"
        );
    }
}

#[cfg(test)]
mod rebound_xp_tests {
    //! ⭐⭐ **THE REBOUND CAST PAID A SECOND, INVENTED XP CREDIT.**
    //! `sub_6AA00` (VA 0x6AA00 = file 0x8F200, up to `sub_6AD60` at
    //! file 0x8F560) holds no `sub_6D8B0` call — the `e8 rel32` scan
    //! of the shipped image puts the nearest callers at 0x8D00C and
    //! 0x8E6E0, both outside it. Spell 8's ONLY award site is the
    //! DEFLECT in `sub_68740` (file 0x8D009 `push $0x8`), which
    //! [`crate::Gen::mc2_rebound_deflect`] already mails. See
    //! [`super::no_mc2_rebound_xp_on_deflect`].
    //!
    //! ⛔ UNGRADED (a WIZEXT lane, book re-imported at every anchor).
    use crate::engine::features::{FeatureAssets, Planes};
    use crate::engine::world::Hx;
    use crate::engine::world::{PlayerCommand, PlayerPose, World};
    use crate::ids::GameId;
    use crate::mc1::mobs::PLAYER_TARGET;
    use crate::mc2::spells::{MC2_SPELL_ROWS, Mc2SpellRow};

    fn away() -> PlayerPose {
        PlayerPose::from_tiles(10.0, 105.0 / 8.0, 10.0, 0.0, 0.0, 0.0)
    }

    fn rebound_world() -> (World, usize) {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let mut spells = vec![Mc2SpellRow::default(); MC2_SPELL_ROWS];
        spells[8].byte_0 = 3;
        for t in 0..3 {
            spells[8].tiers[t].mana_cost = 1_000;
            spells[8].tiers[t].word_0x18 = 125;
            spells[8].tiers[t].max_mana_limit = 0;
            spells[8].tiers[t].life = 0;
        }
        let assets = FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: Vec::new(),
            build_dat: Vec::new(),
            bldgprm: Vec::new(),
            spells,
            mc2_sprite_ext: Vec::new(),
            mc1_sprite_ext: Vec::new(),
        };
        let mut w = World::new_for_game(planes, &[], 1, assets, GameId::Mc2);
        w.mc2_grant_plausible(Hx::FIRST, &[(8, 0)]);
        let m = w.mc2_book[Hx::FIRST].ent[8] as usize;
        assert!(m != 0, "rebound was granted");
        w.mc2_book[Hx::FIRST].left = 8;
        let (px, py, _) = w.human_pose(Hx::FIRST);
        let b = w.g.spawn_mana_ball(px, py, 4000).expect("purse");
        w.g.ent[b].f140 = 200_000;
        w.g.ent[b].f144 = PLAYER_TARGET;
        for _ in 0..70 {
            w.tick(away(), PlayerCommand::default());
        }
        w.set_human_mana(Hx::FIRST, w.human_mana_max(Hx::FIRST));
        assert_eq!(w.g.ent[m].f26, 0, "the window is not armed yet");
        assert_eq!(w.g.ent[m].f54, 0, "the grant cooldown has run off");
        (w, m)
    }

    /// The cast arms the window and pays NOTHING; the deflect mail —
    /// `mc2_rebound_deflect`'s own `(PLAYER_TARGET, 8, 1)`, the port's
    /// home for `sub_68740` file 0x8D00C — is the single credit.
    ///
    /// POSITIVE CONTROL: the deflect mail still lands (+1), so this
    /// cannot pass on a port that has lost the spell-8 award
    /// altogether.
    #[test]
    fn a_rebound_cast_pays_no_xp_and_the_deflect_pays_exactly_one() {
        let (mut w, m) = rebound_world();
        w.tick(
            away(),
            PlayerCommand {
                fire_left: true,
                ..Default::default()
            },
        );
        assert!(w.g.ent[m].f26 > 0, "the rebound window armed");
        assert!(
            w.players[Hx::FIRST].rebound,
            "and the window flag is stamped"
        );
        assert_eq!(
            w.mc2_book[Hx::FIRST].xp_vol[8],
            0,
            "sub_6AA00 has no sub_6D8B0 call — the cast pays no XP"
        );
        for _ in 0..8 {
            w.tick(away(), PlayerCommand::default());
        }
        assert_eq!(
            w.mc2_book[Hx::FIRST].xp_vol[8],
            0,
            "and no later window tick pays one either"
        );

        // POSITIVE CONTROL — the deflect's own mail, the one site
        // retail does award from.
        w.g.mc2_cast_xp.0.push((PLAYER_TARGET, 8, 1));
        w.tick(away(), PlayerCommand::default());
        assert_eq!(
            w.mc2_book[Hx::FIRST].xp_vol[8],
            1,
            "sub_68740 file 0x8D009 `push $0x8` is the ONE award site"
        );
    }
}

#[cfg(test)]
mod replay_xp_gate_tests {
    //! ⭐ THE CAMPAIGN-REPLAY XP GATE (round 147, dig w147g;
    //! [`super::no_mc2_replay_xp_gate`]).
    //!
    //! `sub_6D8B0` (EF:58596) opens with
    //! `if (!(x_D41A0_BYTEARRAY_4_struct.setting_38545 & 4)) { … }` —
    //! the whole body, XP write included, is inside that test. Shipped
    //! `NETHERW.EXE` file 0x920B0 (VA 0x6D8B0):
    //! `mov 0x41a4,%eax` / `testb $0x4,0x9691(%eax)` / `jne 0x6d9b3`
    //! (the epilogue) — BEFORE the `a1` null test at 0x6D8CE and
    //! before the `class==3 && model==0` guard at 0x6D8E3/0x6D8ED. So
    //! on a level this save has ALREADY COMPLETED nothing happens: no
    //! `xp_vol` write, no spell-2 `SetSpell_6D5E0` re-sync, no
    //! `sub_6D9C0` relevel/notify.
    //!
    //! ⛔ UNGRADED: the flag comes out of the capture and the book is
    //! re-imported at every anchor, so no recording fixture can hold
    //! this. A unit test is the lane.
    use crate::engine::features::{FeatureAssets, Planes};
    use crate::engine::world::Hx;
    use crate::engine::world::World;
    use crate::ids::GameId;
    use crate::mc1::mobs::PLAYER_TARGET;

    fn mc2_world(replayed: bool) -> World {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let assets = FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: Vec::new(),
            build_dat: Vec::new(),
            bldgprm: Vec::new(),
            spells: Vec::new(),
            mc2_sprite_ext: Vec::new(),
            mc1_sprite_ext: Vec::new(),
        };
        let mut w = World::new_for_game(planes, &[], 1, assets, GameId::Mc2);
        w.set_mc2_level_replayed(replayed);
        w
    }

    /// A replayed level accrues NO spell XP — and the castle latch
    /// clear still fires, because it sits ABOVE the gate.
    ///
    /// The clear is `sub_60480`'s own `byte_0x1BE_446 = 0` (EF:61992),
    /// three lines ABOVE its `sub_6D8B0` call and therefore outside
    /// the gate; the ONLY producer of a `(PLAYER_TARGET, 2, 1)` award
    /// is `Gen::mc2_castle_upgrade`'s own mail, so the join is exact.
    ///
    /// POSITIVE CONTROL: the same award on a FIRST-PLAY world credits
    /// the XP, which stops this passing for a port that never awards.
    #[test]
    fn a_replayed_level_accrues_no_spell_xp_but_still_clears_the_recast_latch() {
        // POSITIVE CONTROL: first play — the award lands.
        let mut w = mc2_world(false);
        w.mc2_award_xp(PLAYER_TARGET, 7, 5);
        assert_eq!(
            w.mc2_book[Hx::FIRST].xp_vol[7],
            5,
            "a FIRST-PLAY level credits the award"
        );

        // The gate: a level the save has already completed.
        let mut w = mc2_world(true);
        w.mc2_award_xp(PLAYER_TARGET, 7, 5);
        assert_eq!(
            w.mc2_book[Hx::FIRST].xp_vol[7],
            0,
            "a REPLAYED level accrues nothing — sub_6D8B0's first line"
        );

        // …and the latch clear is above the gate, so it still runs on
        // a replayed level.
        for replayed in [false, true] {
            let mut w = mc2_world(replayed);
            w.mc2_recast_surcharge[Hx::FIRST] = true;
            w.mc2_award_xp(PLAYER_TARGET, 2, 1);
            assert!(
                !w.mc2_recast_surcharge[Hx::FIRST],
                "the castle level-up latch clear sits ABOVE the gate (replayed={replayed})"
            );
            assert_eq!(
                w.mc2_book[Hx::FIRST].xp_vol[2],
                if replayed { 0 } else { 1 },
                "…while the XP half of the SAME call obeys it (replayed={replayed})"
            );
        }
    }

    /// ⭐⭐⭐ `sub_6D8B0` HAS NO LIFE GATE — a DEAD human still accrues
    /// spell XP. The shipped function (`NETHERW.EXE` file
    /// 0x920B0..0x921B7) tests the replayed-level bit, `owner != 0`,
    /// `class3f == 3` and `model40 == 0`, and reads nothing else off
    /// the owner record. See
    /// [`crate::mc2::cast::no_mc2_xp_award_while_dead`].
    ///
    /// POSITIVE CONTROL: the same award on a LIVE world credits the
    /// same amount, so a port that never awards cannot pass this.
    /// REVERSION: with `MGC_NO_MC2_XP_AWARD_WHILE_DEAD=1` the dead
    /// arms below go back to 0 and this test FAILS — asserted
    /// unconditionally on purpose, so the switch is a real pre-fix
    /// binary.
    #[test]
    fn a_dead_human_still_accrues_spell_xp() {
        // POSITIVE CONTROL — alive.
        let mut w = mc2_world(false);
        w.set_human_state(Hx::FIRST, crate::engine::world::LifeState::Alive);
        w.mc2_award_xp(PLAYER_TARGET, 9, 1);
        assert_eq!(
            w.mc2_book[Hx::FIRST].xp_vol[9],
            1,
            "a LIVE human credits the award"
        );

        for state in [
            crate::engine::world::LifeState::Falling,
            crate::engine::world::LifeState::Dead,
        ] {
            let mut w = mc2_world(false);
            w.set_human_state(Hx::FIRST, state);
            w.mc2_award_xp(PLAYER_TARGET, 9, 1);
            assert_eq!(
                w.mc2_book[Hx::FIRST].xp_vol[9],
                1,
                "sub_6D8B0 has no life test ({state:?})"
            );
        }
    }
}

#[cfg(test)]
mod invis_grace_tests {
    use crate::engine::features::{FeatureAssets, Planes};
    use crate::engine::world::Hx;
    use crate::engine::world::{PlayerCommand, PlayerPose, World};
    use crate::ids::GameId;
    use crate::mc1::mobs::PLAYER_TARGET;
    use crate::mc2::spells::{MC2_SPELL_ROWS, Mc2SpellRow};

    fn away() -> PlayerPose {
        PlayerPose::from_tiles(10.0, 105.0 / 8.0, 10.0, 0.0, 0.0, 0.0)
    }

    /// A flat MC2 world holding INVISIBILITY (11) at tier 0, with the
    /// row synthesized the way [`super::shield_billing_tests`] does
    /// Shield's — `maxManaLimit` pinned to 0 so `sub_68D50`'s castle
    /// upkeep leg is out of the picture, and a long window so the
    /// first-tick arm is the only edge in play.
    fn invis_world() -> (World, usize) {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let mut spells = vec![Mc2SpellRow::default(); MC2_SPELL_ROWS];
        spells[11].byte_0 = 3;
        for t in 0..3 {
            spells[11].tiers[t].mana_cost = 1_000;
            spells[11].tiers[t].word_0x18 = 301;
            spells[11].tiers[t].max_mana_limit = 0;
            spells[11].tiers[t].life = 1;
        }
        let assets = FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: Vec::new(),
            build_dat: Vec::new(),
            bldgprm: Vec::new(),
            spells,
            mc2_sprite_ext: Vec::new(),
            mc1_sprite_ext: Vec::new(),
        };
        let mut w = World::new_for_game(planes, &[], 1, assets, GameId::Mc2);
        w.mc2_grant_plausible(Hx::FIRST, &[(11, 0)]);
        let m = w.mc2_book[Hx::FIRST].ent[11] as usize;
        assert!(m != 0, "invisibility was granted");
        w.mc2_book[Hx::FIRST].left = 11;
        // Fund the purse the supported way (see the shield rig).
        let (px, py, _) = w.human_pose(Hx::FIRST);
        let b = w.g.spawn_mana_ball(px, py, 4000).expect("purse");
        w.g.ent[b].f140 = 200_000;
        w.g.ent[b].f144 = PLAYER_TARGET;
        for _ in 0..70 {
            w.tick(away(), PlayerCommand::default());
        }
        w.set_human_mana(Hx::FIRST, w.human_mana_max(Hx::FIRST));
        assert_eq!(w.g.ent[m].f26, 0, "the window is not armed yet");
        assert_eq!(w.g.ent[m].f54, 0, "the grant cooldown has run off");
        (w, m)
    }

    /// ⭐⭐ `sub_6B1C0`'s cloak edge writes THREE registers on the
    /// CASTER and the port modelled only two: the missing one is
    /// `word_0x159_345 = 0` — the spawn-grace / at-castle immunity
    /// counter. Shipped `NETHERW.EXE` 0x8FA46
    /// `movw $0x0,0x159(%eax)`, between the afford test at 0x8F9F3
    /// and the first-tick test at 0x8FA07, with `%eax` loaded from
    /// `0xa4(%esi)` = the CASTER's player block (`%esi` =
    /// `Entities[parentId]`).
    ///
    /// ⚠ UNIT, NOT FIXTURE: `grace` is a Type_160 lane the graded
    /// obs never projects. Witness: mc2l6-rival-spells-galore free
    /// run, wiz 0 `invuln` t=18721 / 18900 / 19048, retail 0 port 1 —
    /// three rows, all gone. Fails under
    /// `MGC_NO_MC2_INVIS_CLEARS_GRACE=1`.
    #[test]
    fn going_invisible_zeroes_the_casters_spawn_grace() {
        let (mut w, m) = invis_world();
        // A fresh respawn's window (`sub_5C950` arms 100).
        w.players[Hx::FIRST].grace = 100;
        w.tick(
            away(),
            PlayerCommand {
                fire_left: true,
                ..Default::default()
            },
        );
        assert!(w.g.ent[m].f26 > 0, "fixture: the cloak window opened");
        assert!(w.human_invisible(Hx::FIRST), "fixture: the cloak edge ran");
        // Pre-dig (`MGC_NO_MC2_INVIS_CLEARS_GRACE=1`) the mailbox
        // block's own `--` is the only mover and this reads 99.
        assert_eq!(
            w.vitals(Hx::FIRST).grace,
            0,
            "the cloak edge's third write is `word_0x159_345 = 0`"
        );
    }
}

#[cfg(test)]
mod scatter_window_tests {
    use crate::engine::features::{FeatureAssets, Planes};
    use crate::engine::world::Hx;
    use crate::engine::world::{PlayerCommand, PlayerPose, World};
    use crate::ids::GameId;
    use crate::mc2::spells::{MC2_SPELL_ROWS, Mc2SpellRow};

    fn flat_mc2() -> World {
        let planes = Planes {
            height: vec![100; 0x10000],
            tile_type: vec![5; 0x10000],
            shading: vec![32; 0x10000],
            angle: vec![5; 0x10000],
            ceiling: Vec::new(),
        };
        let mut spells = vec![Mc2SpellRow::default(); MC2_SPELL_ROWS];
        // SPEED UP (3) at tier 0, a long window.
        spells[3].byte_0 = 3;
        for t in 0..3 {
            spells[3].tiers[t].mana_cost = 1_000;
            spells[3].tiers[t].word_0x18 = 301;
            spells[3].tiers[t].max_mana_limit = 0;
        }
        let assets = FeatureAssets {
            rings: (0..32).map(|_| vec![(15u8, 15u8)]).collect(),
            build_tab: Vec::new(),
            build_dat: Vec::new(),
            bldgprm: Vec::new(),
            spells,
            mc2_sprite_ext: Vec::new(),
            mc1_sprite_ext: Vec::new(),
        };
        World::new_for_game(planes, &[], 1, assets, GameId::Mc2)
    }

    /// ⭐⭐ THE DEATH SCATTER MAKES NO STORE TO `word_0x2E_46`.
    ///
    /// `sub_5E310`'s 26-token loop (EF:60533-56) writes exactly four
    /// things per owned manifestation — `byte[0] &= 0xFE`,
    /// `actionIndex_0x45_69++`, the ±256 position, `life_0x8 =
    /// rand % 0x5A + 200` — and the book entry becomes the boolean 1.
    /// Shipped `NETHERW.EXE` file 0x82B10 (linear 0x5E310): the loop
    /// body runs 0x82CFF `andb $0xfe,0xc(%edx) / incb 0x45(%edx)`
    /// through 0x82DAB `mov %edx,0x8(%esi)` and 0x82DBC
    /// `cmpl $0x1a,-0x8(%ebp)` with **no `+0x2e` operand anywhere in
    /// the function**. The window counter simply FREEZES at whatever
    /// the last live tick left it, because the record now dispatches
    /// the pickup state instead of the effect body that owns the
    /// trailing `dec` (`GetScroll_69DB0`, file 0x8E78C).
    ///
    /// The port's `mc2_scatter_spells` zeroed it. Census: `(15,3) f2e`
    /// 242/264/274 rows on mc2l23 / mc2l19-taketwo / mc2l22-new, one
    /// human-death scatter per take, retail holding a flat 299 / 286 /
    /// 257 against the port's 0. The RIVAL twin
    /// (`mc2/rivals.rs`, the death payout) already had this right.
    ///
    /// `MGC_NO_MC2_SCATTER_KEEPS_WINDOW=1` restores the clear and
    /// FAILS this test.
    #[test]
    fn the_death_scatter_leaves_the_window_counter_standing() {
        let mut w = flat_mc2();
        w.mc2_grant_plausible(Hx::FIRST, &[(3, 0)]);
        let m = w.mc2_book[Hx::FIRST].ent[3] as usize;
        assert!(m != 0, "Speed Up was granted");
        assert_eq!(w.g.ent[m].tick70, 9, "the owned state is 3M");
        // An ACTIVE Speed Up window, two ticks in.
        w.g.ent[m].f26 = 299; // word_0x2E_46
        w.g.ent[m].f28 = 301; // word_0x30_48
        w.g.ent[m].flags |= 1; // byte[0] bit 0 — in the book

        let corpse = PlayerPose::from_tiles(40.0, 40.0, 12.0, 0.0, 0.0, 0.0);
        w.mc2_scatter_spells(Hx::FIRST, corpse);

        assert_eq!(
            w.mc2_book[Hx::FIRST].ent[3],
            1,
            "the book entry is the boolean marker (EF:60146)"
        );
        assert_eq!(
            w.g.ent[m].tick70, 10,
            "`actionIndex_0x45_69++` — the loose pickup state 3M+1"
        );
        assert_eq!(w.g.ent[m].flags & 1, 0, "`byte[0] &= 0xFE`");
        assert!(
            (200..290).contains(&w.g.ent[m].act_life),
            "`life_0x8 = rand % 0x5A + 200`, got {}",
            w.g.ent[m].act_life
        );
        assert_eq!(
            w.g.ent[m].f26, 299,
            "`sub_5E310` makes NO store to +0x2e — the counter stands"
        );

        // …and it is INERT: the effect body is book-and-state gated,
        // so the frozen counter never counts down or re-fires.
        for _ in 0..4 {
            w.tick(corpse, PlayerCommand::default());
        }
        assert_eq!(
            w.g.ent[m].f26, 299,
            "the loose jar's window neither ticks nor resumes"
        );
    }
}
