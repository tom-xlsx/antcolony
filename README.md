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

(Text snapshot from `--headless`: `.` tunnel, `o` chamber, `%` stored food, `a` ant. The real
thing is in colour.)

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

A nest is one connected tunnel system. Workers eat a little from the chambers all year round
and four times as much in a drought, so stores only build when foraging outpaces eating. A nest
grows one worker at a time, and only once it holds a reserve for the workers it already has,
so stockpiles come before mouths; that is the only way the world gains ants. In a drought a
nest with nothing left loses a worker every half day until the drought ends or nobody is left.
Tuned so that, on a 120×40 pane, restocking after a drought takes about a hundred days of the
hundred and twenty between droughts, and roughly a third of droughts arrive before it is done. The status line shows `DRY SPELL`,
`DROUGHT day 14/21` and `BLOOM`, and counts starving and dead nests. All the knobs (day
length, drought spacing and length, consumption, starvation, spawn threshold) are constants
at the top of `src/sim.rs`; `--headless 120000 --report` is the tuning loop.

Future hooks, deliberately not built: a queen replacing the growth rule, brood and castes,
raiding between nests, dormancy during droughts.

`src/sim.rs` is the simulation and has no terminal code in it. `src/ui.rs` paints a world into a
ratatui buffer. `src/main.rs` is the event loop and flags.

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo run --release -- --headless 3000 --seed 1
```

## Licence

MIT.
