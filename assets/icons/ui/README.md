# UI icons

Small UI icons, taken from [Lucide](https://lucide.dev/)
(<https://github.com/lucide-icons/lucide>), licensed under the ISC License, with Feather-derived icons also under the MIT
License (upstream LICENSE kept verbatim in `licenses/LUCIDE-ISC.txt`). They are not covered by CayenChat's GPL-3.0-only
license. The app icon `assets/icons/cayenchat.svg` is separate.

Rules (decision D039): copy only the SVGs that are used, keep the Lucide file
name, add one row below in the same commit, and do not add icons ahead of use.
Prefer Lucide; only if it has nothing suitable use another source or draw one
and record that here.

| File | Lucide name | Lucide version | Used for |
| --- | --- | --- | --- |
| `plug-zap.svg` | `plug-zap` | 1.56.0 | Toolbar: connect |
| `refresh-cw.svg` | `refresh-cw` | 1.56.0 | Toolbar: reconnect |
| `unplug.svg` | `unplug` | 1.56.0 | Toolbar: disconnect |
| `log-in.svg` | `log-in` | 1.56.0 | Toolbar: join the channel |
| `log-out.svg` | `log-out` | 1.56.0 | Toolbar: leave the channel |
| `chevrons-down.svg` | `chevrons-down` | 1.56.0 | Toolbar: next unread channel |
| `user-search.svg` | `user-search` | 1.56.0 | Toolbar: WHOIS |
| `image-up.svg` | `image-up` | 1.56.0 | Toolbar: upload an image |
| `settings.svg` | `settings` | 1.56.0 | Toolbar: settings |

The files are unchanged from upstream (`stroke="currentColor"`); the toolbar
draws them as a mask in the theme's text color, so they show in both light and
dark. They are compiled into the binary (`toolbar::Icons`), so a new icon is
also added to that list.
