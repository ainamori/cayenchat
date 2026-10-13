#!/usr/bin/env python3
"""A scripted demo conversation for store and README screenshots
(spec/development.md, "Store screenshots").

    demo.py run --app PATH [--language english|japanese] [--theme light|dark]
    demo.py serve --port PORT --images DIR [--language ...]
    demo.py settings --port PORT [--language ...] [--theme ...]

`run` does everything on macOS, Windows and Linux desktops: it writes
throwaway settings and images under --dir, starts the demo IRC server on
127.0.0.1, launches a `demo` build of CayenChat (cargo feature `demo`) and
waits until the conversation has been played. Take the screenshots, then press
Enter to quit. `serve` and `settings` are the two halves, for the Xvfb flow
with scripts/e2e/gui_session.py.

The server speaks just enough IRC for the demo: registration, CAP with
server-time and draft/metadata-2 (avatars), JOIN with topics and member lists,
then fixed PRIVMSG lines with today's local times. It accepts one client,
listens only on 127.0.0.1 and never reaches another host. Every image
(avatars and the photo) is drawn by this script, so nothing with a license of
its own is involved, and the app reads them from --dir through the
`preview-fixture` part of the `demo` feature instead of the network.

Only the Python standard library is used.
"""
import argparse
import asyncio
import colorsys
import datetime
import hashlib
import json
import math
import os
import struct
import subprocess
import sys
import threading
import zlib

SETTINGS_VERSION = 15
SERVER = "demo.cayenchat.test"
IMAGES = "https://images.cayenchat.test/"
PHOTO = "lake.png"

# ---------------------------------------------------------------- the script
# Per language: our nickname, the network's name, channels (first one is
# shown) and private messages. Lines are (nickname, text). Received CTCP
# ACTION is not rendered yet (spec/feature-parity.md), so the script has no
# `/me`. Nicknames prefixed with @ or + are operators or voiced
# in the member list. Users in NO_AVATAR show the app's own default avatar.
SCRIPTS = {
    "english": {
        "nick": "sam",
        "network": "DemoNet",
        "channels": [
            {
                "name": "#cayenchat",
                "topic": "CayenChat - a modern IRC client | 1.0 is out!",
                "members": ["@alice", "@bob", "+carol", "+dave", "erin", "frank", "grace", "heidi",
                            "ivan", "judy", "kate", "leo", "mia", "noah"],
                "lines": [
                    ("alice", "Evening, all! Has anyone tried the new release yet?"),
                    ("bob", "Yes, avatars now show up on our old bouncer too."),
                    ("carol", "Same here, and my old ISO-2022-JP channel works fine."),
                    ("dave", "The view from my trip last weekend: " + IMAGES + PHOTO),
                    ("erin", "Beautiful! Is that Lake Kawaguchi?"),
                    ("dave", "It is. The preview loaded right in the log."),
                    ("frank", "sam: could you check the release notes before we announce? "
                              "https://github.com/koshian/cayenchat/releases/tag/v1.0.0"),
                    ("grace", "Long links are shortened in the log, but copying keeps the full URL."),
                    ("alice", "Nice. I switched to the dark theme last night and kept it."),
                ],
            },
            {
                "name": "#rust",
                "topic": "Rust programming",
                "members": ["@ivan", "judy", "kate", "leo"],
                "lines": [
                    ("ivan", "Has anyone profiled the new borrow checker yet?"),
                    ("judy", "A bit. Compile times look about the same for us."),
                ],
            },
            {
                "name": "#random",
                "topic": "Anything goes",
                "members": ["mia", "noah", "erin"],
                "lines": [
                    ("mia", "sam, are you joining the meetup on Friday?"),
                ],
            },
        ],
        "private": [("erin", "Lunch at noon?")],
    },
    "japanese": {
        "nick": "mika",
        "network": "DemoNet",
        "channels": [
            {
                "name": "#cayenchat",
                "topic": "CayenChat - モダンな IRC クライアント | 1.0 リリースしました!",
                "members": ["@akira", "@yui", "+ken", "+mio", "haru", "taro", "sora", "rin",
                            "nao", "kai", "emi", "jun", "riku", "saki"],
                "lines": [
                    ("akira", "こんばんは。新しいリリース、もう試しました?"),
                    ("yui", "試しました。古いバウンサー経由でもアバターが出るようになってますね"),
                    ("ken", "ISO-2022-JP の古いチャンネルも問題なく読めてます"),
                    ("mio", "週末に行った湖の写真です " + IMAGES + PHOTO),
                    ("haru", "きれい!河口湖ですか?"),
                    ("mio", "そうです。ログの中でそのままプレビューが出るのが便利"),
                    ("taro", "mika さん、告知の前にリリースノートを確認してもらえますか? "
                             "https://github.com/koshian/cayenchat/releases/tag/v1.0.0"),
                    ("sora", "長い URL はログでは短く表示されて、コピーすると元の URL になります"),
                    ("akira", "昨夜ダークテーマに切り替えて、そのまま使ってます"),
                ],
            },
            {
                "name": "#rust-jp",
                "topic": "Rust の話題",
                "members": ["@nao", "kai", "emi", "jun"],
                "lines": [
                    ("nao", "新しい借用チェッカーのプロファイルを取った人いますか?"),
                    ("kai", "少しだけ。うちではコンパイル時間はほぼ同じでした"),
                ],
            },
            {
                "name": "#zatsudan",
                "topic": "雑談",
                "members": ["riku", "saki", "haru"],
                "lines": [
                    ("riku", "mika さん、金曜のオフ会来ますか?"),
                ],
            },
        ],
        "private": [("haru", "お昼一緒にどうですか?")],
    },
}
NO_AVATAR = {"heidi", "noah", "rin", "saki"}


def bare(member):
    return member.lstrip("@+")


def everyone(script):
    names = {bare(member) for channel in script["channels"] for member in channel["members"]}
    names.update(nick for nick, _ in script["private"])
    return sorted(names)


# ----------------------------------------------------------------- the images
def png(width, height, pixel):
    """An RGB PNG; `pixel(x, y)` returns (r, g, b) with 0-255 integers."""
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            rows.extend(pixel(x, y))

    def chunk(kind, data):
        return (struct.pack(">I", len(data)) + kind + data
                + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b""))


def rgb(hue, saturation, value):
    return tuple(round(c * 255) for c in colorsys.hsv_to_rgb(hue % 1.0, saturation, value))


def avatar(name, size=96):
    """A symmetric 5x5 pattern in a color derived from the name."""
    digest = hashlib.sha256(name.encode()).digest()
    hue = digest[0] / 255
    background = rgb(hue, 0.25, 0.97)
    foreground = rgb(hue, 0.65, 0.75)
    cells = [[digest[1 + row * 3 + min(col, 4 - col)] & 1 for col in range(5)] for row in range(5)]
    margin = size // 8
    cell = (size - 2 * margin) / 5

    def pixel(x, y):
        col, row = int((x - margin) // cell), int((y - margin) // cell)
        inside = 0 <= col < 5 and 0 <= row < 5 and x >= margin and y >= margin
        return foreground if inside and cells[row][col] else background

    return png(size, size, pixel)


def photo(width=640, height=400):
    """A lake at sunset: sky, sun, two mountain ridges and their reflection."""
    horizon = int(height * 0.62)
    sun_x, sun_y, sun_r = width * 0.68, height * 0.40, height * 0.09

    def ridge(x, base, amplitude, phase):
        return base - amplitude * (0.6 * math.sin(x / width * 5.1 + phase)
                                   + 0.4 * math.sin(x / width * 11.3 + 2 * phase))

    def scene(x, y):
        if (x - sun_x) ** 2 + (y - sun_y) ** 2 <= sun_r ** 2:
            return (255, 236, 179)
        if y >= ridge(x, horizon - height * 0.04, height * 0.06, 1.7):
            return (58, 52, 92)
        if y >= ridge(x, horizon - height * 0.14, height * 0.10, 0.4):
            return (101, 84, 132)
        t = y / horizon
        return (round(255 - 70 * t), round(170 - 40 * t), round(120 + 60 * t))

    def pixel(x, y):
        if y < horizon:
            return scene(x, y)
        mirrored = 2 * horizon - y - 1
        r, g, b = scene(x + 3 * math.sin(y * 0.35), max(mirrored, 0))
        return (round(r * 0.72), round(g * 0.78), round(b * 0.9))

    return png(width, height, pixel)


def write_images(directory, script):
    os.makedirs(directory, exist_ok=True)
    for name in everyone(script) + [script["nick"]]:
        if name not in NO_AVATAR:
            with open(os.path.join(directory, f"avatar-{name}.png"), "wb") as file:
                file.write(avatar(name))
    with open(os.path.join(directory, PHOTO), "wb") as file:
        file.write(photo())


# ------------------------------------------------------------------ settings
def settings(script, port, language, theme):
    return {
        "version": SETTINGS_VERSION,
        "language": language,
        "theme": theme,
        "linux_display": "x11",
        "credential_backend": "local_file",
        "selected_server": "demo",
        "appearance": {"image_previews": True, "user_avatars": True, "compact_urls": True},
        "servers": [{
            "id": "demo",
            "host": "127.0.0.1",
            "port": port,
            "use_tls": False,
            "verify_tls_certificates": True,
            "encoding": "utf8",
            "nickname": script["nick"],
            "username": script["nick"],
            "display_name": script["network"],
            "channels": ",".join(channel["name"] for channel in script["channels"]),
            "connect_on_startup": True,
            "ircv3": {"message_tags": True, "server_time": True, "batch": True},
        }],
    }


def layout(width, height):
    return {"version": 1, "window": {"x": 40, "y": 40, "width": width, "height": height},
            "maximized": False}


# -------------------------------------------------------------------- server
def timestamp(index):
    """Today at 20:30 local time plus a minute or two per line, as UTC."""
    start = datetime.datetime.now().astimezone().replace(hour=20, minute=30, second=0, microsecond=0)
    moment = start + datetime.timedelta(seconds=97 * index)
    return moment.astimezone(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.000Z")


class Client:
    def __init__(self, script, reader, writer, played):
        self.script = script
        self.reader = reader
        self.writer = writer
        self.played = played
        self.nick = script["nick"]
        self.caps = set()
        self.registered = False
        self.got = {"NICK": False, "USER": False, "CAP": False}
        self.joined = set()

    def send(self, line, tags=None):
        if tags and "server-time" in self.caps:
            line = "@" + ";".join(f"{key}={value}" for key, value in tags.items()) + " " + line
        self.writer.write((line + "\r\n").encode())

    def numeric(self, code, *params):
        *middle, last = params
        self.send(" ".join([f":{SERVER}", code, self.nick, *middle, ":" + last]))

    def prefix(self, nick):
        return f"{nick}!{nick}@{nick}.users.cayenchat.test"

    async def run(self):
        while True:
            raw = await self.reader.readline()
            if not raw:
                return
            line = raw.decode("utf-8", "replace").rstrip("\r\n")
            if line.startswith("@"):
                line = line.split(" ", 1)[1] if " " in line else ""
            words, _, trailing = line.partition(" :")
            parts = words.split()
            if not parts:
                continue
            command, args = parts[0].upper(), parts[1:] + ([trailing] if trailing else [])
            await self.handle(command, args)
            await self.writer.drain()

    async def handle(self, command, args):
        if command == "CAP":
            sub = args[0].upper() if args else ""
            if sub == "LS":
                self.got["CAP"] = True
                self.send(f":{SERVER} CAP * LS :server-time message-tags batch draft/metadata-2")
            elif sub == "REQ":
                wanted = args[-1].split()
                self.caps.update(wanted)
                self.send(f":{SERVER} CAP * ACK :{' '.join(wanted)}")
            elif sub == "END":
                self.got["CAP"] = False
                self.register()
        elif command == "NICK" and args:
            self.nick = args[0]
            self.got["NICK"] = True
            self.register()
        elif command == "USER":
            self.got["USER"] = True
            self.register()
        elif command == "PING":
            self.send(f":{SERVER} PONG {SERVER} :{args[-1] if args else ''}")
        elif command == "JOIN" and args:
            for name in args[0].split(","):
                self.join(name)
        elif command == "METADATA" and len(args) >= 2:
            self.metadata(args)
        elif command == "QUIT":
            self.writer.close()

    def register(self):
        if self.registered or not (self.got["NICK"] and self.got["USER"]) or self.got["CAP"]:
            return
        self.registered = True
        network = self.script["network"]
        self.numeric("001", f"Welcome to {network}, {self.prefix(self.nick)}")
        self.numeric("002", f"Your host is {SERVER}")
        self.numeric("003", "This server is a demo for screenshots")
        self.send(f":{SERVER} 004 {self.nick} {SERVER} demo iow ovbt")
        self.numeric("005", f"NETWORK={network}", "CASEMAPPING=rfc1459", "CHANTYPES=#",
                      "PREFIX=(ov)@+", "CHANMODES=b,k,l,nt", "are supported by this server")
        self.numeric("422", "MOTD File is missing")

    def metadata(self, args):
        target, sub = args[0], args[1].upper()
        if sub == "SUB":
            self.send(f":{SERVER} 770 {self.nick} avatar")
        elif sub == "GET" and target in ("*", self.nick):
            self.send(f":{SERVER} 761 {self.nick} {self.nick} avatar * "
                      f":{IMAGES}avatar-{self.nick}.png")

    def channel(self, name):
        return next((c for c in self.script["channels"] if c["name"].lower() == name.lower()), None)

    def join(self, name):
        channel = self.channel(name)
        if channel is None or name.lower() in self.joined:
            return
        self.joined.add(name.lower())
        # Dated like the conversation, so the log does not start with now.
        self.send(f":{self.prefix(self.nick)} JOIN {channel['name']}", {"time": timestamp(-2)})
        self.numeric("332", channel["name"], channel["topic"])
        names = " ".join(channel["members"] + [self.nick])
        self.send(f":{SERVER} 353 {self.nick} = {channel['name']} :{names}")
        self.numeric("366", channel["name"], "End of /NAMES list")
        if "draft/metadata-2" in self.caps:
            for member in channel["members"]:
                nick = bare(member)
                if nick not in NO_AVATAR:
                    self.send(f":{SERVER} METADATA {nick} avatar * :{IMAGES}avatar-{nick}.png")
        if len(self.joined) == len(self.script["channels"]):
            asyncio.get_running_loop().create_task(self.play())

    async def play(self):
        # The shown channel's lines come last so that its log is the newest,
        # and the other channels' lines leave unread marks.
        await asyncio.sleep(1.0)
        index = 0
        for channel in self.script["channels"][1:] + self.script["channels"][:1]:
            for nick, text in channel["lines"]:
                self.say(nick, channel["name"], text, index)
                index += 1
        for nick, text in self.script["private"]:
            self.say(nick, self.nick, text, index)
            index += 1
        await self.writer.drain()
        self.played.set()

    def say(self, nick, target, text, index):
        self.send(f":{self.prefix(nick)} PRIVMSG {target} :{text}", {"time": timestamp(index)})


async def serve(script, port, played, ready=None):
    async def accept(reader, writer):
        await Client(script, reader, writer, played).run()

    server = await asyncio.start_server(accept, "127.0.0.1", port)
    if ready:
        ready.set()
    async with server:
        await server.serve_forever()


# ------------------------------------------------------------------ commands
def script_for(args):
    return SCRIPTS[args.language]


def command_serve(args):
    script = script_for(args)
    write_images(args.images, script)
    played = threading.Event()
    print(f"demo server on 127.0.0.1:{args.port}", flush=True)
    try:
        asyncio.run(serve(script, args.port, played))
    except KeyboardInterrupt:
        pass


def command_settings(args):
    print(json.dumps(settings(script_for(args), args.port, args.language, args.theme), indent=2,
                     ensure_ascii=False))


def private_directory(path):
    os.makedirs(path, exist_ok=True)
    if os.name == "posix":
        os.chmod(path, 0o700)


def command_run(args):
    script = script_for(args)
    directory = os.path.abspath(args.dir or os.path.join(
        os.path.dirname(os.path.abspath(__file__)), "..", "..", "target", "tmp",
        f"screenshots-{args.language}-{args.theme}"))
    config = os.path.join(directory, "settings")
    images = os.path.join(directory, "images")
    private_directory(directory)
    private_directory(config)
    write_images(images, script)
    width, height = (int(value) for value in args.size.split("x"))
    with open(os.path.join(config, "settings.json"), "w", encoding="utf-8") as file:
        json.dump(settings(script, args.port, args.language, args.theme), file, indent=2,
                  ensure_ascii=False)
    with open(os.path.join(config, "window.json"), "w", encoding="utf-8") as file:
        json.dump(layout(width, height), file, indent=2)

    played, ready = threading.Event(), threading.Event()
    thread = threading.Thread(target=lambda: asyncio.run(serve(script, args.port, played, ready)),
                              daemon=True)
    thread.start()
    if not ready.wait(10):
        sys.exit(f"the demo server could not listen on 127.0.0.1:{args.port}")

    env = dict(os.environ, CAYENCHAT_TEST_DIR=config, CAYENCHAT_PREVIEW_FIXTURE_DIR=images)
    log = open(os.path.join(directory, "app.log"), "w")
    app = subprocess.Popen([args.app], env=env, stdin=subprocess.DEVNULL, stdout=log,
                           stderr=subprocess.STDOUT)
    print(f"CayenChat started (pid {app.pid}); settings in {config}", flush=True)
    while not played.wait(1):
        if app.poll() is not None:
            sys.exit(f"CayenChat exited; see {directory}/app.log")
    print("The demo conversation has been played. Take the screenshots now.")
    if sys.platform == "darwin":
        print(f"  window ID: swift scripts/screenshots/window_id.swift {app.pid}")
        print("  then: screencapture -o -l <ID> shot.png")
    try:
        input("Press Enter to quit CayenChat. ")
    except (EOFError, KeyboardInterrupt):
        pass
    app.terminate()
    try:
        app.wait(10)
    except subprocess.TimeoutExpired:
        app.kill()


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)

    def common(command, port=True):
        command.add_argument("--language", default="english", choices=sorted(SCRIPTS))
        if port:
            command.add_argument("--port", type=int, default=36690)

    command = commands.add_parser("run", help="start the server and a demo build, then wait")
    common(command)
    command.add_argument("--app", required=True,
                         help="a CayenChat binary built with --features demo")
    command.add_argument("--theme", default="light", choices=["light", "dark"])
    command.add_argument("--size", default="1280x800",
                         help="window size in logical pixels (restored from window.json)")
    command.add_argument("--dir", help="working directory (default target/tmp/screenshots-...)")
    command.set_defaults(func=command_run)

    command = commands.add_parser("serve", help="only run the demo server")
    common(command)
    command.add_argument("--images", required=True, help="directory to write the images to")
    command.set_defaults(func=command_serve)

    command = commands.add_parser("settings", help="print settings.json for the demo server")
    common(command)
    command.add_argument("--theme", default="light", choices=["light", "dark"])
    command.set_defaults(func=command_settings)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
