# antcolony

An ant colony that lives in a terminal pane.

Ants wander out from the nest, find food, carry it home and lay a pheromone trail on the way.
Other ants follow the trail, reinforce it, and a path forms. Trails evaporate when the food runs
out, so paths fade and new ones appear somewhere else. A well-fed colony grows, its nest gets
bigger, and when it gets crowded a founding party marches off together to dig a new nest.

Nothing to configure. Drop it in a zellij or tmux pane and leave it running.

```
                      ..:::::###########:::...
                    ..::##aaaa####aaa#####::..        FFFF
                   .:::##a##:::::::::###aa##::......FFFFFF
                  ..::#a#::...........:::#aa####aaaaFFFF
                 ..::#a#::..     NNNNNN    ...:::::###::.
                 .::#a##:.     NNNNNNNNNN
                 .::#a#::.    NNNNNNNNNNNN
```

(Text snapshot from `--headless`. The real thing is in colour: amber trails toward food, blue
trails toward home, green food, earth-brown nests.)

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
| `f` | drop a food pile somewhere random |
| `h` | hide or show the status line |
| left click | drop a food pile right there |

Resizing the pane starts a fresh colony at the new size.

## Flags

```
--ants N        starting ants (default scales with the pane, 20 to 300)
--speed 1..8    simulation speed (default 2)
--seed N        same seed and pane size gives the same colony
--food N        food piles kept in the world at once (default 3)
--max-nests N   most nests allowed (default 4)
--no-bud        crowded nests stay put instead of founding new ones
--no-grow       fixed population; nests never grow or bud
--quiet, -q     start with the status line hidden
--headless T    run T ticks with no terminal and print a text snapshot
--size CxR      world size for --headless, in terminal cells (default 71x24)
```

## How it works

Each terminal cell is two stacked pixels (`▀` with separate foreground and background colours),
so a 71×24 pane is a 71×48 world. Two pheromone grids cover it: one laid by foraging ants that
says "a nest is this way", one laid by returning ants that says "food is this way". Ants sense
the relevant grid at three points ahead and turn toward the strongest. Both grids evaporate a
little every tick and blur a little every few ticks. That is the whole trick; everything you see
falls out of it.

`src/sim.rs` is the simulation and has no terminal code in it. `src/ui.rs` paints a world into a
ratatui buffer. `src/main.rs` is the event loop and flags.

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo run --release -- --headless 3000 --seed 1
```

## Licence

MIT.
