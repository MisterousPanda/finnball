using UnityEngine;

namespace Finnball
{
    public static class FinnballCourtBuilder
    {
        public static GameObject Build(FinnballManifest data, string arenaId, Transform parent = null)
        {
            var arena = data.Arena(arenaId);
            var court = data.court;
            var root = new GameObject($"FINNBALL_{arena.id}");
            if (parent != null)
                root.transform.SetParent(parent, false);

            var floor = Quad(
                "CourtFloor",
                new Vector3(court.half_length * 2f, 0.08f, court.half_width * 2f),
                new Vector3(0f, -0.04f, 0f),
                FinnballColor.Parse(arena.palette.floor_a),
                root.transform
            );
            floor.transform.localRotation = Quaternion.identity;

            var paint = FinnballColor.Parse(arena.palette.paint);
            var line = FinnballColor.Parse(arena.palette.line);
            foreach (var hoop in data.hoops)
            {
                var sign = hoop.position.x > 0f ? 1f : -1f;
                var paintX = hoop.position.x - sign * (court.paint_depth * 0.5f);
                Box(
                    $"Paint_{hoop.id}",
                    new Vector3(court.paint_depth, 0.02f, court.paint_half_width * 2f),
                    new Vector3(paintX, 0.01f, hoop.position.z),
                    paint,
                    root.transform
                );
                var boardX = hoop.position.x + sign * court.backboard_offset;
                Box(
                    $"Backboard_{hoop.id}",
                    new Vector3(0.06f, 1.05f, 1.8f),
                    new Vector3(boardX, hoop.position.y + 0.15f, hoop.position.z),
                    line,
                    root.transform
                );
                Box(
                    $"Pole_{hoop.id}",
                    new Vector3(0.12f, hoop.position.y, 0.12f),
                    new Vector3(boardX + sign * 0.35f, hoop.position.y * 0.5f, hoop.position.z),
                    new Color(0.12f, 0.12f, 0.14f),
                    root.transform
                );
                var rim = GameObject.CreatePrimitive(PrimitiveType.Cylinder);
                rim.name = $"Rim_{hoop.id}";
                rim.transform.SetParent(root.transform, false);
                rim.transform.localPosition = hoop.position.ToUnity();
                rim.transform.localScale = new Vector3(
                    court.rim_radius * 2f,
                    0.02f,
                    court.rim_radius * 2f
                );
                Tint(rim, new Color(1f, 0.45f, 0.08f));
            }

            var ball = GameObject.CreatePrimitive(PrimitiveType.Sphere);
            ball.name = "Ball";
            ball.transform.SetParent(root.transform, false);
            ball.transform.localPosition = data.spawns.ball_home.ToUnity();
            ball.transform.localScale = Vector3.one * (court.ball_radius * 2f);
            Tint(ball, new Color(0.85f, 0.35f, 0.12f));

            foreach (var team in data.teams)
            {
                var spots = team.id == "home" ? data.spawns.home : data.spawns.away;
                var color = FinnballColor.Parse(team.primary);
                for (var i = 0; i < spots.Length; i++)
                {
                    var player = GameObject.CreatePrimitive(PrimitiveType.Capsule);
                    player.name = $"Player_{team.id}_{i}";
                    player.transform.SetParent(root.transform, false);
                    player.transform.localPosition = spots[i].ToUnity() + Vector3.up * 0.94f;
                    player.transform.localScale = new Vector3(
                        court.player_radius * 2f,
                        0.94f,
                        court.player_radius * 2f
                    );
                    Tint(player, color);
                }
            }

            if (Camera.main != null)
                Camera.main.backgroundColor = FinnballColor.Parse(arena.palette.sky);

            return root;
        }

        static GameObject Box(string name, Vector3 size, Vector3 center, Color color, Transform parent)
        {
            var go = GameObject.CreatePrimitive(PrimitiveType.Cube);
            go.name = name;
            go.transform.SetParent(parent, false);
            go.transform.localPosition = center;
            go.transform.localScale = size;
            Tint(go, color);
            return go;
        }

        static GameObject Quad(string name, Vector3 size, Vector3 center, Color color, Transform parent)
        {
            return Box(name, size, center, color, parent);
        }

        static void Tint(GameObject go, Color color)
        {
            var renderer = go.GetComponent<Renderer>();
            if (renderer == null)
                return;
            renderer.sharedMaterial = new Material(renderer.sharedMaterial) { color = color };
        }
    }
}
