# antcolony

An ant colony that lives in a terminal pane, seen side-on like a glass ant farm.

Sky on top, soil below. Ants dig shafts and tunnels, hollow out chambers, and pile the spoil
into little mounds by the entrance. Foragers go up to the surface, find food, lay a trail
for the others and carry it down into the chambers, which slowly fill with green. Stored food
feeds new ants, more ants dig more tunnels, and once the colony is big enough a small party
walks off along the surface and opens a second nest.

Then the seasons turn. Every so often a dry spell sets in, the sky goes dusty, a sun comes out
and the surface food stops. Each nest lives off what it stored: well-stocked nests ride it out,
small or careless ones starve, lose workers and die, leaving their tunnels as a grey husk for
the next colony to break into. When the rain comes back there is a bloom, survivors regrow,
and the question is whether they stock enough before the next one.

Nothing to configure. Drop it in a zellij or tmux pane and leave it running.

```
                a                          a
               a#a    F               F   a#a
     a   a    a###a  a      a   a    a   a###a      a
####a#########.####################a#######.##################
##############.############################.##################
##############.######........##############.##################
##############.######.########....#########.##################
#######........######.###########.#########....###############
#######.######.######.###########.#############.##############
#######.####ooooo####.######%%%%%##############.##############
#######.###ooooooo###.#####%%%%%%%#############.##############
```

(Text snapshot from `--headless`: `.` tunnel, `o` room, `%` stored food, `R` royal chamber,
`Q` queen, `e` egg, `+` corpse, `f n d s` ants by job, `x` a starving one. The real thing is
in colour.)

## The queen, jobs and hunger

Every nest has one queen. She starts in the first room the nest digs, never leaves it except
once or twice in her life, and every so often lays an egg if she is well fed and the larder
would see the nest through twelve days of drought; the egg hatches into a worker a few
hundred ticks later. That is the only way the world gains ants. If she
dies the nest is doomed: no more eggs, and once the rooms are eaten bare the workers walk off
to join the nearest nest that still has a queen. Her corpse lies in the chamber for a while.
A queenless nest crumbles slowly back into soil from its tips inward, entrance last, so the
ground is free for whoever comes next; living nests keep well clear of a decaying nest and
only dig into that ground once it has gone. If the whole farm goes queenless for fifty days, a
founding queen flies in from outside, digs a short shaft and lays from her own reserve until
she has a few workers.

Workers have jobs, not castes. A job is reassigned when the nest's larder changes, one ant at
a time so you can watch the workforce pivot.

| Job | Colour | Does |
|---|---|---|
| Forager | ivory | works the surface near home, follows trails, hauls food (pale gold while carrying) |
| Nurse | pink | keeps to the rooms, feeds the queen and hungry diggers, minds the stores |
| Digger | tan | extends tunnels and rooms; digs the royal chamber when the nest is ready |
| Scout | pale blue | ranges the whole surface, sees further, lays the strong trail, leads founding parties (orange on the march) |

| Larder | forager | nurse | digger |
|---|---|---|---|
| plentiful | 40% | 30% | 30% |
| normal | 60% | 30% | 10% |
| scarce, or any drought | 85% | 15% | 0% |

Scouts take a tenth of the forager share. "Plentiful" means the store would last more than
fifteen days of drought; "scarce" fewer than five. A nest that has no queen yet puts everyone
on digging until its first room is open.

Every ant has a belly. Foragers eat on the surface or at the store, nurses from the rooms they
stand in, diggers are fed by nurses or, if nobody comes, walk to the larder. When the queen
runs low a couple of nurses fetch a unit each from the nearest room that has any and carry it
to her (pale gold while carrying). Stored food is the floor of a room itself, shading from
brown toward green as it fills.
Hunger runs six times faster in a drought. An ant that runs empty drops its job and its load,
goes dim, then dark red, and dies: on the surface after a few steps, underground on reaching
a room. Corpses lie where they fell and fade. The queen carries a much larger reserve, so in a
bad drought the workers go before she does; if it goes on long enough she stops laying, then
starves too.

The queen's chamber is outlined in gold in the surrounding dirt. Once a nest is deep and its
larder plentiful, it looks for a suitable deeper room and a few diggers prepare it, enlarging
it into a hall about twice the size of a room (a warmer shade marks the earth still to dig).
When it is ready the queen walks down to it, slowly, nurses trailing, and settles there. The
old chamber keeps its outline; she moves at most twice, and the nest keeps digging below her.
When her line dies the outlines fade away with the nest.

The status line shows the jobs as `f66 n12 d30 s12`, the egg count, and nest flags such as
`1 queenless`, `1 preparing a hall`, `1 moving`. When a drought ends a small box reports how
long it lasted and how many ants and queens were lost since the last one; it closes by itself.

## Install

Needs a Rust toolchain and a terminal with true colour (iTerm2, Ghostty, kitty, WezTerm, …).

```bash
cargo install --path .
```

That puts `antcolony` in `~/.cargo/bin`.

## Keys

| Key | Does |
|---|---|
| `q` `Esc` `Ctrl-C` | quit |
| `space` | pause |
| `r` | reset with a new seed |
| `+` `-` | faster / slower (1 to 8) |
| `f` | drop a food pile somewhere on the surface |
| `h` | hide or show the status line |
| left click | drop a food pile on the surface at that column |

Resizing the pane starts a fresh colony at the new size.

## Flags

```
--ants N        starting ants (default scales with the pane, 20 to 300)
--speed 1..8    simulation speed (default 2)
--seed N        same seed and pane size gives the same colony
--food N        food piles kept on the surface at once (default 3)
--max-nests N   most nest entrances allowed (default 4)
--no-bud        a big colony never sends out a party to open a new nest
--no-grow       fixed population; stored food never becomes new ants
--no-drought    endless calm: no dry spells, droughts or blooms
--report        with --headless, print one line per drought (for tuning)
--quiet, -q     start with the status line hidden
--headless T    run T ticks with no terminal and print a text snapshot
--size CxR      world size for --headless, in terminal cells (default 71x24)
```

## How it works

Each terminal cell is two stacked pixels (`▀` with separate foreground and background colours),
so a 71×24 pane is a 71×48 world. Underground, ants walk along dug cells and steer by
breadth-first distance fields: one from the entrances (the way out), one from chambers with
room (where to put food), one from tunnel tips (where to dig next). Diggers keep a two-cell
wall around every tunnel so the farm stays readable, and stop once about a fifth of the soil
is hollow. On the surface a one-dimensional trail of pheromone leads foragers to food.

A nest is one connected tunnel system. Each living nest may hollow out a share of the soil
(two-fifths of it between them; a lone nest gets a fifth), so diggers work until that is used
and then idle in the tunnels, widening the busiest corridors to three cells as they loiter,
filling gaps and extending runs before starting new ones so the wide stretches stay smooth. Food goes into rooms; every ant eats from it; the queen turns a
full belly into eggs. Tuned so that on a 120×40 pane most droughts kill a nest or two and
none wipe the farm, and restocking takes a good part of the gap between droughts.

Future hooks, deliberately not built: brood as entities, soldiers and raiding, a replaceable
queen, corpses as food or disease, a second relocation that must also be bigger.

`src/sim.rs` is the simulation and has no terminal code in it. `src/ui.rs` paints a world into a
ratatui buffer. `src/main.rs` is the event loop and flags.

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo run --release -- --headless 3000 --seed 1
```

## Licence

MIT.
