# FINNBALL Blender add-on

Install `blender/finnball` as a Blender 4.2+ add-on (Preferences → Add-ons → Install from Disk, pick this folder or zip it). Then:

- **File → Import → FINNBALL Manifest (.json)** and choose `interop/finnball.json`
- or run **FINNBALL: Load repo manifest** from the F3 search if this repo is next to the add-on

The add-on builds the 3v3 court (floor, paints, hoops, three-point discs), a ball, and six placeholder players at the Bevy tip-off spots. Arena palettes come from the JSON. Coordinates are converted from Bevy Y-up metres to Blender Z-up metres.

Export the collection as glTF if you want the same meshes in Unity or Unreal (`File → Export → glTF 2.0`).
