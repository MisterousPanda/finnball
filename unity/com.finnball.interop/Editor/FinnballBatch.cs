using System;
using System.IO;
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;
using UnityEngine.SceneManagement;

namespace Finnball.Editor
{
    /// <summary>
    /// Headless entry points for the `unity` CLI:
    ///
    ///   unity run unity/Finnball -- -executeMethod Finnball.Editor.FinnballBatch.BuildCourtScene -finnballArena neo_tokyo
    ///   unity build unity/Finnball --target WebGL --execute-method Finnball.Editor.FinnballBatch.BuildWebGL -o "$PWD/unity/Finnball/Build/WebGL"
    ///
    /// Note: the CLI resolves a relative -o against its own working directory, so pass an absolute path.
    /// </summary>
    public static class FinnballBatch
    {
        const string ScenesDir = "Assets/Scenes";
        const string ScenePath = ScenesDir + "/Court.unity";

        public static void BuildCourtScene()
        {
            var arena = Arg("-finnballArena") ?? "neo_tokyo";
            SyncManifest();
            var data = FinnballManifest.FromJson(FinnballManifestLocator.ReadText());

            var scene = EditorSceneManager.NewScene(NewSceneSetup.DefaultGameObjects, NewSceneMode.Single);
            var root = FinnballCourtBuilder.Build(data, arena);
            root.AddComponent<FinnballRuntimeBoot>();

            var cam = Camera.main;
            if (cam != null)
            {
                // Broadcast-ish sideline framing, same side the Bevy default cam uses.
                cam.transform.position = new Vector3(0f, 13f, -24f);
                cam.transform.LookAt(new Vector3(0f, 1.0f, 0f));
                cam.fieldOfView = 52f;
                cam.clearFlags = CameraClearFlags.SolidColor;
                cam.backgroundColor = FinnballColor.Parse(data.Arena(arena).palette.sky);
            }

            Directory.CreateDirectory(ScenesDir);
            EditorSceneManager.SaveScene(scene, ScenePath);
            AddSceneToBuildSettings(ScenePath);
            AssetDatabase.SaveAssets();
            Debug.Log($"FINNBALL: saved {ScenePath} for arena {arena}");
        }

        public static void BuildWebGL()
        {
            var output = Arg("-buildOutput") ?? "Build/WebGL";
            if (!File.Exists(ScenePath))
                BuildCourtScene();
            SyncManifest();

            PlayerSettings.WebGL.compressionFormat = WebGLCompressionFormat.Brotli;
            PlayerSettings.WebGL.decompressionFallback = true;
            PlayerSettings.WebGL.template = "PROJECT:Finnball";
            if (!Directory.Exists("Assets/WebGLTemplates/Finnball"))
                PlayerSettings.WebGL.template = "APPLICATION:Default";
            PlayerSettings.runInBackground = true;

            var options = new BuildPlayerOptions
            {
                scenes = new[] { ScenePath },
                locationPathName = output,
                target = BuildTarget.WebGL,
                options = BuildOptions.None,
            };
            var report = BuildPipeline.BuildPlayer(options);
            var summary = report.summary;
            Debug.Log($"FINNBALL WebGL: {summary.result}, {summary.totalSize / (1024 * 1024)} MB, {summary.totalTime}");
            if (summary.result != UnityEditor.Build.Reporting.BuildResult.Succeeded)
                throw new Exception($"FINNBALL WebGL build failed: {summary.result}");
        }

        /// <summary>Copies interop/finnball.json into StreamingAssets so players can read it.</summary>
        public static void SyncManifest()
        {
            var repo = FinnballManifestLocator.RepoPath();
            if (repo == null)
                throw new FileNotFoundException("interop/finnball.json not found above the Unity project");
            var dst = Path.Combine(Application.streamingAssetsPath, FinnballManifestLocator.StreamingName);
            Directory.CreateDirectory(Path.GetDirectoryName(dst));
            File.Copy(repo, dst, true);
            AssetDatabase.Refresh();
        }

        static void AddSceneToBuildSettings(string path)
        {
            foreach (var s in EditorBuildSettings.scenes)
            {
                if (s.path == path)
                    return;
            }
            var list = new System.Collections.Generic.List<EditorBuildSettingsScene>(EditorBuildSettings.scenes)
            {
                new EditorBuildSettingsScene(path, true),
            };
            EditorBuildSettings.scenes = list.ToArray();
        }

        static string Arg(string name)
        {
            var args = Environment.GetCommandLineArgs();
            for (var i = 0; i < args.Length - 1; i++)
            {
                if (args[i] == name)
                    return args[i + 1];
            }
            return null;
        }
    }
}
