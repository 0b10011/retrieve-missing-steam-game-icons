# Retrieve missing steam game icons

Windows script to download missing icons for steam game shortcuts in the current directory.

Usage:

```powershell
cd ~\Desktop
~\Downloads\retrieve-missing-steam-game-icons.exe
```

## How it works

1. Extracts steam game ID and icon filename from all `*.url` files in the current directory
2. Checks if the game already has an icon downloaded; if so, continues onto the next
3. Downloads the icon from Steam's CDN (`https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/{game_id}/{icon_filename}`)
4. Saves the icon to Steam's local icon folder (`C:\Program Files (x86)\Steam\steam\games\`)

Note: You'll likely need to refresh any pages containing shortcuts with broken icons before the changes will show up.

## Why was this made?

My OS's SSD died.
The SSD with my games on it was fine.
I replaced the OS drive,
reinstalled Windows and Steam,
and added shortcuts to my desktop for all of my games.
But none of them had an icon.
So, I wrote this script.
