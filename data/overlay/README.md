# sc2-replay-utils — overlay templates

This folder **is** your overlay. Everything in it is served by the app over
`http://127.0.0.1:<port>/` while the overlay server is running (default port
`8722`, Settings → Stream overlay).

You do not need to know Rust, and you never need to rebuild the app. Edit
these files with any text editor, save, refresh the page — done.

---

## Quick start

1. Settings → Stream overlay → **Enable overlay server**, then **Copy URL**.
2. In OBS: Sources → **+** → **Browser** → paste the URL.
   - Compact bar at `/` — suggested size **700 x 200**.
   - Stats panel at `/stats-dashboard.html` — suggested size **340 x 620**.
   - Live players at `/live-players.html` — suggested size **760 x 150**.

   Sizing generously is free: the empty area is transparent, while a source
   that is too small clips the layout.

   Settings → Stream overlay lists every view it finds here — including the
   ones you write — each with a button to copy its URL or open it in your
   browser.
3. Open this folder (Settings → **Open template folder**) and edit. Where to
   go depends on what you want to change:
   - **a colour** → `tokens.css`, and every overlay follows
   - **how one block looks** → its rule in `components.css`
   - **which blocks a page shows** → the `{% include %}` lines in that page
   - **a whole new overlay** → see [Building your own](#building-your-own-overlay)
4. Refresh the page to see your change (see [Live reload](#live-reload) —
   edits do *not* refresh by themselves).

Broke something? Settings → **Restore default template** puts the shipped
files back and keeps yours as `.bak`.

---

## What is in this folder

Nothing here is a monolith. There are **pieces**, and there are **pages that
assemble pieces** — so changing one thing means opening one small file.

**Pages** — what you point OBS at. Each is a dozen lines: which pieces, in
which order.

| File | Served at |
|---|---|
| `index.html` | `/` — the compact bar |
| `stats-dashboard.html` | `/stats-dashboard.html` — the panel |
| `live-players.html` | `/live-players.html` — who is playing right now |

**Pieces** — one block each, in `partials/`. Drop any of them into any page.

| File | What it shows |
|---|---|
| `partials/session-inline.html` | Today's score on one line: `TODAY 3–2 60% 4210 +42` |
| `partials/session-card.html` | The same numbers as a donut + tally panel |
| `partials/last-game-inline.html` | The last game on one line |
| `partials/last-game-card.html` | The last game as a two-line card |
| `partials/form-strip.html` | Ten blocks of recent form, newest on the left |
| `partials/race-grid.html` | Today's record by opponent race |
| `partials/live-versus.html` | The two players currently on screen |
| `partials/nickname-hint.html` | "Add your nickname" — renders nothing once you have |

Each carries its own empty state, so you never need an `{% if %}` around
one, and each is a valid page on its own: `/partials/form-strip.html` in an
OBS source gives you just that strip.

**Skeleton and helpers**

| File | What it is |
|---|---|
| `base.html` | The page skeleton every page extends: `<head>`, the stylesheets, the live-reload pair |
| `macros.html` | Small snippets — `race_icon(race)`, `signed(number)` |

**Styling** — three layers, so you rarely touch more than one.

| File | What it is |
|---|---|
| `tokens.css` | The palette. **Change a colour here and every overlay follows.** |
| `components.css` | How the pieces look. One block per piece. |
| `style.css`, `stats-dashboard.css`, `live-players.css` | Just the arrangement of each page — a dozen lines each |

**Assets**: `race/*.svg` (the icons the app itself uses) and this `README.md`.
Anything else you drop here is served too — your logo, a webfont, a sound.
Subfolders work (`img/logo.png` → `/img/logo.png`).

---

## Building your own overlay

Save this as `mine.html` in this folder, and it is live at `/mine.html`:

```jinja
{% extends "base.html" %}
{% block title %}My overlay{% endblock %}
{% block content %}
  {% include "partials/live-versus.html" %}
  {% include "partials/session-inline.html" %}
{% endblock %}
```

That is a complete, styled overlay that reloads itself. Extending
`base.html` is what gives you the palette, the piece styling and the
live-reload script without writing a line of either.

The blocks you can fill in:

| Block | What goes in it |
|---|---|
| `title` | The browser tab / OBS source title |
| `styles` | Extra `<link>` or `<style>` for this page only |
| `body_class` | A class on `<body>`, if your CSS wants one |
| `content` | The page itself |

Add a `{% block styles %}<style> … </style>{% endblock %}` to restyle a
piece for your page only, or edit `components.css` to restyle it everywhere.

---

## How a page is rendered

- **`.html` files are templates.** They are rendered with
  [minijinja](https://docs.rs/minijinja), which speaks the Jinja2 syntax you
  know from Flask/Django/Ansible: `{{ value }}`, `{% if %}`, `{% for %}`,
  `{{ x | upper }}`.
- **Templates compose**: `{% extends %}` with `{% block %}` for the
  skeleton, `{% include %}` to drop a piece in, `{% import %}` for macros.
  Paths are relative to this folder and always use `/`, even on Windows.
- **Everything else is served verbatim** — CSS, JS, images, fonts, video. No
  templating, no processing.
- Every file is re-read from disk **on every page load**. There is no cache
  to clear and no restart to do, for pages and pieces alike.
- HTML is auto-escaped. A player named `<script>` cannot break your layout.

`/` maps to `index.html`. **Any** shipped file you delete — a page, the
skeleton, a piece — falls back to the copy built into the app, so a stray
delete can never leave you with a blank overlay mid-stream. A *syntax
error* is not covered by that on purpose: it shows the error page naming
the file and line, instead of silently serving the default and letting you
believe your edits are live.

---

## Live reload

The page reloads itself **when the data changes** — a new replay lands, a
scan finishes, you edit your nicknames. That is what these two lines at the
bottom of `index.html` do, and why you should keep them in any layout you
write:

```html
<script>
  window.__overlayRev = {{ revision }};
</script>
<script src="/_overlay/live.js"></script>
```

The first line tells the script which revision the page was rendered with, so
an update landing while the page loads is not missed. The second polls the
app and reloads when the number changes.

**Editing a template does not trigger a reload** — the data did not change.
Refresh manually: F5 in a browser, or in OBS right-click the source →
*Refresh* (or Properties → *Refresh cache of current page*).

The one exception: if your template has an error, the error page retries
every 2 seconds, so fixing the file brings the overlay back on its own.

---

## The data your template sees

Everything below is available as a top-level variable. To see the **live**
values with your own games in them, open in a browser:

```
http://127.0.0.1:8722/_overlay/data.json
```

That URL is the authoritative reference — this list is here so you know what
to look for.

### Top level

- `revision` — number, bumped on every data update. Used by the live-reload
  script.
- `generated_at` — `"YYYY-MM-DDTHH:MM:SS"`, when this data was published.
- `nicknames_configured` — `false` until you add a nickname in Settings.
  While it is false the app cannot tell which player is you, so `session` is
  all zeros and `me`/`opponent` are empty. Branch on it instead of showing
  silent zeros.
- `session` — today's score. See below.
- `last_game` — the most recent game, or `none`. Same shape as an item of
  `recent_games`; it *is* `recent_games[0]`.
- `recent_games` — up to **10** games, newest first.
- `live` — who is playing *right now*. See below.

### `session` — today only

- `session.date` — `"YYYY-MM-DD"`.
- `session.games` — decided games today (`wins + losses`).
- `session.wins`, `session.losses`.
- `session.winrate` — `0.0`–`1.0`, or `none` with no decided game.
- `session.winrate_pct` — the same thing as `0`–`100`, already rounded.
- `session.mmr_latest` — your MMR in today's most recent game, or `none`.
- `session.mmr_delta` — today's swing: last MMR minus first. `none` with
  fewer than two MMR readings today.
- `session.by_race` — today's record split by **opponent** race. Always four
  entries, always in the order Zerg, Terran, Protoss, Random, even when
  zeroed — so you can draw the full grid without checking for gaps. Each
  entry: `race`, `race_letter`, `games`, `wins`, `losses`, `winrate_pct`.

### A game (`last_game`, each of `recent_games`)

- `map` — map name.
- `datetime` — `"YYYY-MM-DDTHH:MM:SS"`; `date` and `time` (`"HH:MM"`) are the
  same value pre-split.
- `is_today` — `true` when this game counts toward `session`.
- `matchup` — `"ZvT"`, your race first. Empty when no nickname is configured.
- `result` — from your point of view: `"Win"`, `"Loss"`, `"Undecided"`, or
  `""` when no nickname is configured. **All four cases happen** — vs-AI
  games and disconnects really do report `Undecided` — so if you use it as a
  CSS class, style them all.
- `duration_seconds` and `duration_label` (`"12:34"`; minutes are not
  zero-padded: `"7:03"`, `"61:01"`).
- `me` / `opponent` — `{ name, race, race_letter, mmr, result }`, or `none`
  when no nickname is configured.
- `players` — both players in replay order, regardless of nicknames. Use this
  when you want the raw scoreboard.

### `live` — the game on screen right now

Everything else on this page comes from your replay folder, so it can only
describe games that already **finished**. `live` is the exception: every
couple of seconds the app asks the StarCraft II client itself who is on
screen.

- `live.connected` — `false` when StarCraft II is not running (or is still
  starting up). Everything else in `live` is empty then.
- `live.in_game` — `true` when a **1v1 between two humans** is on screen.
- `live.players` — those two players, in the order the client reports them,
  each with `name`, `race` and `race_letter`. Empty unless `live.in_game`.

`live.in_game` is deliberately narrow, matching the cut the rest of the
overlay uses: games against the AI, FFA, 2v2 and replay playback all read as
*not in a game*. One difference you should know about: the client does not
say whether a match is ladder, so a custom 1v1 against a friend does count
here, while it would never reach `session` or `recent_games`.

There is no result, no clock and no match state — those come from the replay
once the game is over, which is the source that can be trusted. Note also
that `live.players` are the names as the client reports them, unrelated to
the **Nickname on stream** setting below; `me`/`opponent` exist only on
finished games.

The page updates by itself: a match starting bumps the revision, which is
what the live-reload script watches.

### Two things worth knowing

**Only ladder 1v1 games ever reach the overlay.** Customs, arcade and games
against the AI are filtered out, with no setting to change it: the session
score, the MMR swing and the record by race are ladder numbers, and one
practice game would quietly corrupt them on stream.

**`session` is today; `last_game` and `recent_games` are your whole library.**
That is deliberate — before your first game of the day the overlay still has
something to show, and `is_today` lets you tell the cases apart.

One more, if you play on several accounts: Settings → Stream overlay →
**Nickname on stream** picks which of your nicknames counts as you here.
With one selected, the other accounts drop out of the overlay entirely —
score, MMR and history alike. Left on *All nicknames*, every account you have
registered counts, which is the default.

---

## Starting from scratch instead

Extending `base.html` is a convenience, not a requirement. Any `.html` file
here is a page — `mylayout.html` is served at `/mylayout.html` with the same
data — so you can also write one top to bottom and share nothing:

```html
<!doctype html>
<html>
  <head>
    <meta charset="utf-8" />
    <style>
      body { margin: 0; background: transparent; color: #fff;
             font: 700 42px "Segoe UI", sans-serif; }
    </style>
  </head>
  <body>
    {{ session.wins }} – {{ session.losses }}
    <!-- Doing it this way, these two lines are yours to remember. -->
    <script>window.__overlayRev = {{ revision }};</script>
    <script src="/_overlay/live.js"></script>
  </body>
</html>
```

That is the whole trade-off: a standalone page owes nobody anything, but the
live reload, the palette and the piece styling are then on you. Mixing is
fine too — extend `base.html` and ignore the pieces, or include one piece
into a page you wrote yourself.

Run as many as you like at once: a bar on top, a panel in the corner, a
"waiting" screen for your break — one OBS Browser source each.

---

## Static files (images, fonts, sounds)

Copy the file here and reference it with a path from the root:

```html
<img src="/img/logo.png" />
<link rel="stylesheet" href="/fonts.css" />
```

Rules the server applies:

- Files larger than **64 MB** are not served.
- Folders are never listed; a URL that points at a folder returns 404.
- Paths cannot escape this folder (`..` is rejected, symlinks pointing
  outside are rejected).
- A few names are refused because they are hazardous on Windows: device
  names (`CON`, `NUL`, `COM1`…`LPT9`), names containing `:` or `\`, and names
  ending in a space or a dot.
- Unknown extensions are sent as `application/octet-stream`, which browsers
  download instead of displaying. Common web types (html, css, js, json, svg,
  png, jpg, gif, webp, avif, ico, woff/woff2, ttf, otf, mp4, webm, mp3, ogg,
  wav, txt, md) are typed correctly.

Local files are the safe choice. A CDN URL will usually work — OBS has
internet — but it breaks your overlay the day the CDN is slow.

---

## Reserved URLs

The `/_overlay/` prefix belongs to the app. A file you place in an
`_overlay/` subfolder here is unreachable — put your assets anywhere else.

- `/_overlay/live.js` — the live-reload script.
- `/_overlay/revision` — the current revision number (plain text). Supports
  `?since=N&wait=1` for long polling; that is how `live.js` avoids busy
  polling.
- `/_overlay/data.json` — the full data your template receives, pretty
  printed. Nothing in the app consumes it; it exists so you can look.

---

## When something breaks

**A red "OVERLAY TEMPLATE ERROR" page.** Your template has a syntax or
rendering error. The page names the file and line, and shows the offending
source — and since pages are assembled from pieces, that file may be a
partial rather than the page you opened. Fix it and save; the page comes
back on its own.

**404 not found.** The file is not in this folder, the name does not match
exactly (case included, on Linux), or it hit one of the naming rules above.

**403 forbidden.** The overlay only answers requests addressed to
`localhost` / `127.0.0.1`. It is not reachable from another machine — that is
intentional, and it is also what keeps the Windows Firewall dialog away.

**A black rectangle in OBS instead of your gameplay.** Something is painting
a background. Keep `body { background: transparent; }` and give panels their
own semi-transparent background instead.

**Zeros everywhere / no player names.** No nickname configured (Settings →
Nicknames), or your library has no ladder 1v1 replay yet.

**Nothing changes after you edit a file.** Expected — refresh the page (see
[Live reload](#live-reload)). If OBS still shows the old version, use
*Refresh cache of current page* in the source properties.

**The server will not start.** The port is taken by another program. Change
it in Settings, click *Restart server*, and update the URL in OBS — OBS
stores the full URL, so it does not follow the change by itself.

---

## Restoring the defaults

Settings → Stream overlay → **Restore default template** rewrites every file
listed in [What is in this folder](#what-is-in-this-folder) with the version
shipped in the app — pages, pieces, skeleton, stylesheets and this README.
Any of them you had edited is renamed to `<name>.bak` first, so nothing is
lost, and files you added yourself are left alone.

To undo one bad edit you do not need any of that: delete the file and the
app serves its built-in copy again on the next reload.

---

## Jinja notes

The template language is Jinja2 as implemented by minijinja. Two things that
catch people out:

**`none` is not `0` or `""`.** Optional values (`winrate_pct`, `mmr_latest`,
`mmr_delta`, `me`, `opponent`, `last_game`) can be `none`, and printing one
gives you the text `none`. Guard them:

```jinja
{% if session.mmr_latest is not none %}{{ session.mmr_latest }}{% endif %}
```

Note that `{% if session.wins %}` is false at zero — use `is not none` when
zero is a value you want to show.

**Filters bind tighter than you expect — but looser than arithmetic.**
`10 - recent_games | length` parses as `(10 - recent_games) | length`. Use
parentheses:

```jinja
{% for _ in range(10 - (recent_games | length)) %}
```

Useful bits used by the shipped templates:

```jinja
{{ last_game.result | lower }}                     {# CSS class #}
{{ last_game.result | upper }}
{{ '+' if session.mmr_delta > 0 else '' }}{{ session.mmr_delta }}
{% for game in recent_games %} … {% endfor %}
{% for r in session.by_race %}{% if r.games %} … {% endif %}{% endfor %}
{{ '%.1f' | format(value) }}
```
