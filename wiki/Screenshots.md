# Screenshots

All images are rendered by OpenBlood from a retail copy of Blood. The level shots were made
with the `shotg` developer tool at 1600×900 (see [Developer Tools](Developer-Tools)), the
menus in the running game with F12.

## Levels

### Episode 1: The Way of All Flesh

E1M2, Shipwrecked:

![E1M2](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/map_e1m2.png)

E1M4, The Haunting (translucent fire sprites and masked walls):

![E1M4](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/map_e1m4.png)

### Episode 2: Even Death May Die

E2M1:

![E2M1](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/map_e2m1.png)

E2M4 (axe zombies in the corridor):

![E2M4](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/map_e2m4.png)

![Episode 2](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/episode2.png)

![Episode 2, another view](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/episode2_2.png)

### Episode 3: Farewell to Life

E3M1 (city streets, parallax sky):

![E3M1](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/map_e3m1.png)

E3M5:

![E3M5](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/map_e3m5.png)

![Episode 3](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/episode3.png)

### Episode 4: Dead Reckoning

E4M2:

![E4M2](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/map_e4m2.png)

E4M9 (the axe-zombie ambush used as a combat reference in the tests):

![E4M9](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/map_e4m9.png)

## Interface

Main menu, drawn with the original fonts and art:

![Main menu](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/main_menu.png)

Options:

![Options](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/options.png)

Help:

![Help](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/help.png)

Credits:

![Credits](https://raw.githubusercontent.com/dagrigorev/OpenBlood/master/screenshots/credits.png)

## Making your own

In the game, press **F12**: the next frame is saved as `openblood_NNNN.png` in the current directory.

From the command line (no window needed):

```
OB_NOMSG=1 openblood shotg <DATA> E3M1 e3m1.png 1920 1080 0.5
```

renders E3M1 at 1920×1080 after half a second of simulation, with the HUD.
