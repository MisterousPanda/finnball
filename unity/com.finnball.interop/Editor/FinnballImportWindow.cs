using System.IO;
using UnityEditor;
using UnityEngine;

namespace Finnball.Editor
{
    public class FinnballImportWindow : EditorWindow
    {
        string _manifestPath;
        string _arena = "neo_tokyo";
        static readonly string[] Arenas =
        {
            "neo_tokyo",
            "toon_world",
            "sky_temple",
            "underground",
            "crystal_coliseum",
        };

        [MenuItem("FINNBALL/Import Court")]
        public static void Open()
        {
            var window = GetWindow<FinnballImportWindow>("FINNBALL");
            window._manifestPath = GuessManifest();
            window.Show();
        }

        void OnGUI()
        {
            EditorGUILayout.LabelField("Manifest", EditorStyles.boldLabel);
            EditorGUILayout.BeginHorizontal();
            _manifestPath = EditorGUILayout.TextField(_manifestPath);
            if (GUILayout.Button("…", GUILayout.Width(28)))
            {
                var picked = EditorUtility.OpenFilePanel("FINNBALL manifest", _manifestPath, "json");
                if (!string.IsNullOrEmpty(picked))
                    _manifestPath = picked;
            }
            EditorGUILayout.EndHorizontal();

            _arena = Arenas[EditorGUILayout.Popup("Arena", IndexOf(_arena), Arenas)];

            if (GUILayout.Button("Build court in scene"))
                Build();
        }

        void Build()
        {
            if (!File.Exists(_manifestPath))
            {
                EditorUtility.DisplayDialog("FINNBALL", $"Missing {_manifestPath}", "OK");
                return;
            }
            var data = FinnballManifest.FromJson(File.ReadAllText(_manifestPath));
            var root = FinnballCourtBuilder.Build(data, _arena);
            Undo.RegisterCreatedObjectUndo(root, "Import FINNBALL court");
            Selection.activeGameObject = root;
        }

        static int IndexOf(string arena)
        {
            for (var i = 0; i < Arenas.Length; i++)
            {
                if (Arenas[i] == arena)
                    return i;
            }
            return 0;
        }

        static string GuessManifest()
        {
            var dir = new DirectoryInfo(Application.dataPath);
            while (dir != null)
            {
                var candidate = Path.Combine(dir.FullName, "interop", "finnball.json");
                if (File.Exists(candidate))
                    return candidate;
                dir = dir.Parent;
            }
            return "interop/finnball.json";
        }
    }
}
