# Shortwave Web Player

A retro 80s-style VFD radio interface for the Shortwave P2P internet radio service.

## Features

- VFD (Vacuum Fluorescent Display) aesthetic with cyan glow
- Dot matrix font display
- Tune through stations with arrow buttons
- Real-time now-playing metadata via SSE
- Volume control
- Minimalistic interface focused on the display and controls

## Development

```bash
npm install
npm run dev
```

The dev server will proxy API requests to `http://localhost:8080` by default.

## Build

```bash
npm run build
```

The built files will be in the `dist/` directory.

## Usage

1. Start a Shortwave node (see main README)
2. Start the web player dev server
3. Tune through stations using the arrow buttons
4. Press play to start listening
5. Adjust volume as needed

The interface shows:
- Current frequency (like a real radio dial)
- Station name
- Now playing information (artist, title, album)
- Playback status

Navigate stations without seeing the full list - tune through them like a real radio!

