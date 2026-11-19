# circuitCad

**English | [日本語](README.md)**

**Draw a schematic, simulate it, turn it into a PCB, and export manufacturing files, all in one free app built for people learning electronics.**

circuitCad combines schematic capture, SPICE-style simulation, breadboard prototyping, PCB layout, Gerber export and bringing up the board you received in a single desktop application. The screen has four tabs in the order you build: **Schematic → Prototype → PCB layout → Bench**. It is aimed at beginners who want to go from "learning a circuit" to "experimenting with it" to "ordering a real board" and "getting it running" without switching tools.

> **English UI.** The whole interface (menus, hover help, error messages, the operation guide, the glossary, the learning paths and all lessons) can be shown in English: **Settings → Language / 言語 → English**, or start it with `--lang en`. The English text was translated with AI help from the Japanese original; if a sentence reads oddly, please open an issue (or fix the line in [`crates/i18n/en/`](crates/i18n/en/)).
>
> The English mode is included from **`v0.1.0-alpha.11`**; the zips up to `v0.1.0-alpha.10` are Japanese only. In `v0.1.0-alpha.12` the breadboard's hole names (such as `上+1`) are still Japanese in the English UI.

[**Download for Windows**](https://github.com/K2EXTER/circuitCad/releases) (alpha, no installer: unzip and run `circuitCad.exe`)

Project page (features, analyses, known issues and roadmap): <https://k2exter.github.io/circuitCad/en/>

> **Status: early alpha.** Mainly tested on Windows (Linux: lightly checked on WSL2 only). A board made with this software has been manufactured and its copper checked; it has not been assembled and tested yet. See [Limitations](#limitations) before relying on it.

![A series regulator schematic with node voltages and branch currents overlaid on the wires (English UI)](docs/images/en/schematic-dc.png)

| A lesson: predict first, then simulate | The operation guide (F1) |
|---|---|
| ![Lesson 1, "Dividing voltage": the lesson window asks for a prediction before the step is shown](docs/images/en/lesson.png) | ![The operation guide window with mouse and key operations](docs/images/en/guide.png) |

A screen recording (about 80 s, Japanese UI) with one sample, a potentiometer that dims an LED: the DC operating point and the knob turned with the results following, the breadboard switched on and its knob turned, the board imported, routed and checked, the 3D view, then the Bench tab (assembly and first power-on).

![A screen recording (about 80 s): DC results on the schematic, the breadboard running, the board imported, auto-routed and checked by DRC, the 3D view, and the Bench tab's assembly and first power-on. The UI is in Japanese.](docs/images/demo-live.gif)

## Who it is for
- People who are new to electronics and find tools like KiCad or LTspice intimidating.
- People who want to **see a circuit run**: voltages are shown as wire colours and currents as arrows, directly on the schematic.
- People who want to know **why** a value was chosen: resistor and converter calculators show their steps and caveats.
- People who want to take their own design **all the way to an order**.

## Features
- **Beginner-friendly.** Every button and field has a hover explanation. Wiring mistakes and rating violations come with the cause and how to fix it. `F1` opens an operation guide.
- **A guided learning path.** "Build a regulated power supply" takes you from the basics to a board and a real-world measurement: 9 learning paths (power supply, Arduino-compatible board, Mega-compatible board, board craft, switching loads, battery-powered devices, signals, parts, repair), more than 90 step-by-step lessons, 8 board lessons (including repairing a board with a fault put in on purpose), 10 "build it yourself" challenges that check your circuit, and guides for bench experiments. Lessons ask you to predict a result before you simulate it. Digital circuits can be run live: click the switches and watch High / Low colours and the truth table, step one clock at a time, or slow the gate delays down to see a glitch.
- **Try it on a breadboard** (from alpha.12). The Prototype tab lays the schematic's parts out on a breadboard and lights the holes that are connected; drag parts and jumper wires to move them, and leads that differ from the schematic get a red circle. Switch the power on and the circuit runs as it is plugged in: LEDs light, buttons click, a potentiometer turns, a buzzer sounds through your speakers, and parts over their rating get scorched (and fail in the circuit too). A meter and a scope are included.
- **Learn by breaking things.** Open, short, reverse or change parts and pins, and compare how far the damage spreads. Fault diagnosis works the other way: enter the voltages you measured on a circuit that does not work, and it lists the likely causes.
- **Schematic to fabrication files, and on to the bench.** Import into the PCB editor, then place, route (auto-route, push-and-shove, hand routing with cleanup), pour copper, run DRC, view in 3D, run pre-order checks for JLCPCB / PCBWay / OSH Park rules, and export Gerber, drill, BOM and placement files. 2 to 8 copper layers, several boards in one project (checked connections between them, stacked in 3D, simulated together) and STEP export are supported. The Bench tab walks through the board you received: assembly (soldering order and polarity marks as a printable sheet), the first power-on (the normal current range from the schematic and the current limit to set), measuring, pass/fail and repair.
- **Simulation:** DC operating point, transient (adaptive time step, with a logic-analyser style digital view), AC (Bode plot), DC sweep, noise, poles and zeros, root locus, loop gain (phase margin, Nyquist and Nichols plots), impedance seen from a net, THD, CMRR and PSRR, sensitivity, battery life, self-heating (θjc and heatsinks, ambient sweeps, over time), parameter sweep, Monte Carlo (yield and Cpk) and worst-case tolerance analysis, temperature effects, fault simulation. Step-response figures include settling times down to a ±0.003 % band.
- **Heat on the board.** A heat map of the board solved from copper, vias, convection and radiation (also over time, as colours in the 3D view, and inside a closed case), fed back into self-heating; a calculator gives the copper area or heatsink a loss needs.
- **Design calculators and filter design.** LED resistors, dividers, regulators, transistor and MOSFET switches, buck and boost converters, op-amp gain and more, with their steps and caveats. Butterworth, Bessel and Chebyshev filters are sized, placed on the schematic and checked against the target with AC analysis.
- **Cross-checked against ngspice.** Results on the bundled examples are compared with the open-source ngspice (DC operating points and transients; stored answers are checked in CI). Numbers, tolerances and known mismatches are on the [accuracy page](docs/accuracy.md) (Japanese). Agreement with real hardware has not been verified.
- **Parts and samples included.** More than 260 parts (power ICs, sensors, batteries, ADCs, low-leakage analog switches, and real parts from KOA, ROHM, Vishay, Yageo, Panasonic, Murata, Nichicon, Rubycon, Bourns, Kingbright, Infineon and others; series of values are grouped, so many more values are available) and more than 130 sample circuits, including a 41-part mixed-signal demo (power supply + 555 + counter + temperature alarm + amplifier) with a hand-routed board. KiCad symbols, footprints and schematics, and LTspice schematics (.asc), can be imported.
- **Part models from data sheets.** The bundled models are fitted to their data sheets, and the app shows which data sheet each came from and how far it is from the table values. Your own parts can be drafted from a PDF (package, pin table, dimensions, recommended pads, absolute maximum ratings, thermal values), curves traced from its graphs, and scanned pages read with Windows OCR. MOSFETs, transistors, optocouplers, op-amps, regulators and logic ICs from a Boolean expression have quick forms.
- **Headless use (`ccad`).** Build circuits, run ERC, analyses (operating point, transient, AC, noise, sweeps, part-value sensitivity, poles and zeros, loop gain), boards (placement, routing, requirement checks, pictures) and fabrication files from a one-command-per-line script, with every answer as JSON (made for AI agents and automation). See [docs/cli.md](docs/cli.md) (Japanese).
- **A design-requirements table.** Write net roles, widths and currents, parts that belong together or must stay apart, bypass-capacitor pairs and measures on the copper path (resistance, voltage drop, vias, parallel run) in the schematic; the PCB editor uses them for placement, routing and checks, and shows the result row by row.
- **Multi-sheet schematics,** reusable snippets, copy and paste, and wires that follow a part you drag.
- **Free and open source,** under `AGPL-3.0-or-later` (if you distribute a modified version, or let others use it as a network service, publish its source under the same license).

| Prototype (run it on a breadboard) | Bench (first power-on of the board you received) |
|---|---|
| ![The Prototype tab: the schematic's parts plugged into a breadboard with the power on and the LED lit](docs/images/en/prototype.png) | ![The Bench tab: the first power-on steps and the normal current range from the schematic](docs/images/en/bench.png) |

| Transient waveform | Bode plot (AC analysis) |
|---|---|
| ![Transient waveform of a 555 oscillator](docs/images/en/waveform.png) | ![AC analysis Bode plot](docs/images/en/bode.png) |

| Hand-routed board (41 parts, 249 tracks) | 3D view |
|---|---|
| ![PCB editor showing the mixed-signal demo board](docs/images/en/board.png) | ![3D view of the same board](docs/images/en/view3d.png) |

| Pre-order check |
|---|
| ![The "Prepare order" window: fab rules, pre-order checks and their explanations](docs/images/en/order.png) |

| A board with the design requirements imported | The design-requirements table |
|---|---|
| ![PCB editor with frames for parts that go together, lines for distances, and a list of requirements not yet met](docs/images/requirements-board.png) | ![The design-requirements table: net roles, widths and currents, with the reason for each](docs/images/requirements-table.png) |

These two (Japanese UI) show the author's work-in-progress milliohm meter (3 mΩ to 3 MΩ, 10 ranges, several sheets). Write what you want of the circuit in the table, and on the board it becomes checks for parts that go together, distances and track widths (some tracks are still unrouted and some requirements are not met yet). The design is not verified on hardware and uses makers' models, so its files are not included.

## Headless use (`ccad`)
`ccad` runs circuitCad's calculations without opening a window. A script has **one command per line**: build a circuit by part and pin names, run ERC and analyses, make a board and export fabrication files. Every answer is one JSON object per line, so AI agents and automation can use it.

```
new
add V 5 as=V1
add R 270 as=R1
add LED as=D1
net VCC V1.+ R1.1
net LEDA R1.2 D1.A
net GND V1.- D1.K
erc
dc
```

Save this as `led.ccad` and run `ccad led.ccad`. In the answer of `dc`, `elements.D1.current` is the current through the LED (about 11 mA). If ERC still has errors, `circuit_ok` in the answer is `false`, so you know whether to trust the numbers.

- **It is also an MCP server.** Register it with, for example, `claude mcp add ccad -- ccad --strict mcp`, and an AI agent can call the same commands as tools (`--strict` refuses to analyse a circuit that has ERC errors). Board pictures are returned over MCP too.
- **From source:** `cargo run -p cli -- led.ccad`. The release zips up to `v0.1.0-alpha.8` do not contain `ccad`.
- The command list and the caveats are in [docs/cli.md](docs/cli.md) (Japanese).

## Try it in five minutes
1. Download the zip from the [releases page](https://github.com/K2EXTER/circuitCad/releases), unzip it and run `circuitCad.exe`. Windows SmartScreen may warn about the unsigned executable: choose "More info" then "Run anyway". Use `v0.1.0-alpha.11` or later for the English UI.
2. Choose **Settings → Language / 言語 → English**. The setting is kept for the next start.
3. **File → Open sample circuit → "Make 5V from 12V to light an LED"** (a three-terminal regulator circuit).
4. In the left panel, open the **Analysis** tab and press **Run DC operating point**. Voltages appear as wire colours and numbers, currents as arrows.
5. Select a resistor, turn on **Auto recalculate** and drag its value slider; the result updates live.
6. Open **Learn → Learning path** and start with the first lesson.

## Build from source
You need [Rust](https://www.rust-lang.org/).

```bash
cargo run -p app --release
cargo test --workspace
```

Start it in English with `cargo run -p app --release -- --lang en` (or choose the language in the Settings menu once).

On Linux you also need GTK and related development packages (`libgtk-3-dev`, `libxkbcommon-dev`, `libwayland-dev`, ...). A Japanese font is loaded from the system at runtime, and a bundled one (BIZ UDGothic, SIL OFL 1.1) is used if none is found; in English mode Latin text uses Segoe UI on Windows (DejaVu Sans on Linux) when it is there.

A code map for contributors is in [CLAUDE.md](CLAUDE.md), with per-file notes in [docs/code-notes/](docs/code-notes/). These are written in Japanese (code and comments are in English). How the English UI works, and how to add or fix a translation, is in [docs/code-notes/i18n.md](docs/code-notes/i18n.md) (Japanese): the Japanese text stays in the code as the key of the tables in `crates/i18n/en/*.tsv` (one `Japanese<TAB>English` per line).

## Used in
- **[Ladder Lab](https://github.com/K2EXTER/ladder-lab)**: building measuring instruments, power supplies and electronic loads from simple ones up, and using them to improve each other's accuracy. The first instrument, a DMM, was drawn in circuitCad, checked with ERC and the DC operating point, then built on a breadboard and measured (ADC-only noise 1.2 µV). The build log, mistakes included, is in [an article](https://zenn.dev/k2exter/articles/5ff24daec3b92a) (Japanese).

## Limitations
- **Tested mainly on Windows 10 / 11.** On Linux, the author built it, launched it and tried basic operation on WSL2 (Ubuntu 26.04, no GPU) only; other distributions and real hardware are untested, and you may need GTK and related runtime libraries. macOS builds and passes the tests in CI, but nobody has launched the app there yet.
- **Board output is proven only up to a delivered bare board.** A board ordered from this software's Gerber and drill files has been delivered, and its copper pattern was checked with no defects found. It has not been assembled and tested yet. For a first order, use a small quantity and check the result in the fabricator's Gerber viewer.
- **The simulator uses simplified models** and can differ noticeably from real hardware. Each lesson explains the differences.
- **Part data and fabricator rules are approximate.** Always check the datasheet and the fabricator's current page. A part whose land pattern has not been checked against the datasheet is flagged `land_unverified`, and the pre-order check warns about it.
- **Do not use it as the basis for safety-critical designs** (medical, automotive, high voltage, lithium-cell chargers, and so on).
- **The user interface is still being reworked.** It is the area improved most actively, release by release (layout, wording, icons, one consistent look across the editors and windows), but some panels are still dense and some screens do not match the others yet. Feedback on what confused you is welcome.
- **The English text is new** and was translated with AI help; some wording may be awkward, and a few messages built at run time may still appear in Japanese. Reports are welcome.
- It is developed by a small team (mostly one person plus an AI assistant), so replies and fixes can be slow.

Use at your own risk. No guarantee is made that a board designed with this software will work or can be manufactured.

## About how it was built
Almost all of the code was written by Claude (via Claude Code); the maintainer operates the app, runs the tests and decides what to build. The English UI text was also translated by Claude, from the Japanese original. The write-up (in Japanese) is on [Zenn](https://zenn.dev/k2exter/articles/d54c59e62156c3).

## Contributing
Bug reports, suggestions and part additions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). Issues in English are fine.

## License
Copyright (C) 2026 K2EXTER

[GNU Affero General Public License v3.0 or later](LICENSE-AGPL) (`AGPL-3.0-or-later`). You may use, study, modify and share it freely. If you distribute a modified version, or let others use it as a network service, you must publish its whole source to your users under the same license.

The early versions up to `v0.1.0-alpha.7` were published under `MIT OR Apache-2.0`; those terms still apply to those versions (the license texts are in [legacy-licenses/](legacy-licenses/)).

Unless stated otherwise, contributions you submit are licensed under the same license (AGPL-3.0 or later). The bundled BIZ UDGothic font is distributed under the SIL Open Font License 1.1. Third-party Rust crates and their licenses are listed in [THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt).
