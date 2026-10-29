# 0003: OrbStack network bridge off

- Date: 2026-10-29
- Status: accepted
- Topic: fleet-tooling

## Context

OrbStack and Apple `container` both make vmnet bridges (`bridgeNNN`) on the
Mac. OrbStack makes one for each Docker network that the Mac can reach, in a
small set of slots. Other agents' test gates add and remove such networks
every few minutes.

On 2026-10-29, `container` 1.2.2 started a container, destroyed OrbStack's
newest bridge (`bridge108`) and made its own bridge with the same name. Two
minutes later OrbStack removed its orphaned interface (`vmenet8`), and macOS
tore down `bridge108`. The Apple container lost its gateway. configd and
`~/.orbstack/log/vmgr.log` show each step. `container` 1.6.1 did not repeat
it in 7 runs, but no release note names a fix.

## Options

1. Stay on `container` 1.6.1 and detect a lost bridge in the launcher.
2. Turn off OrbStack's `network_bridge`, so only Apple `container` makes
   vmnet bridges.
3. Never run both tools at the same time.

## Choice

The operator chose option 2: `orb config set network_bridge false`.

## Consequences

- The Mac cannot reach OrbStack containers or machines by IP or
  `*.orb.local`. Reach them through published `127.0.0.1` ports or
  `kubectl port-forward`.
- effect-folio's OrbStack proof uses its localhost port-forward mode
  (`http://localhost:8080`). Its HTTPS mode (`https://folio.k8s.orb.local`)
  does not work while the bridge is off.
- Containers reach each other and the internet as before. `lfs-orb` uses
  published ports only.
- Changing the setting restarts OrbStack's Docker engine. Containers without
  a restart policy stop.
