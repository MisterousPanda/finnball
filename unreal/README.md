# FINNBALL Unreal plugin

Open `unreal/Finnball.uproject` in Unreal Engine 5.5 (or copy `Plugins/Finnball` into an existing project). Court numbers live in `FFinnballCourt` and match `src/sim.rs`.

To spawn a full arena from the interchange file, run this in the editor Python console:

```python
import unreal
unreal.execute_python_script(r"/absolute/path/to/finnball/unreal/Scripts/import_finnball.py")
```

Or pass an arena slug: `import_finnball.py sky_temple`.

`UFinnballLibrary::BevyToUnreal` converts Y-up metres into Unreal Z-up centimetres so gameplay ports stay lined up with Bevy.
