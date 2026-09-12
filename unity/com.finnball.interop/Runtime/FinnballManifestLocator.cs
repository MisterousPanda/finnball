using System.IO;
using UnityEngine;

namespace Finnball
{
    /// <summary>
    /// Finds <c>interop/finnball.json</c>. In the editor it walks up from the project folder
    /// (the Unity project lives inside the finnball repo); in a player it reads the copy under
    /// StreamingAssets that <c>FinnballBatch.SyncManifest</c> stages.
    /// </summary>
    public static class FinnballManifestLocator
    {
        public const string StreamingName = "finnball.json";

        public static string RepoPath()
        {
            var dir = new DirectoryInfo(Application.dataPath);
            while (dir != null)
            {
                var candidate = Path.Combine(dir.FullName, "interop", "finnball.json");
                if (File.Exists(candidate))
                    return candidate;
                dir = dir.Parent;
            }
            return null;
        }

        public static string StreamingPath()
        {
            return Path.Combine(Application.streamingAssetsPath, StreamingName);
        }

        public static string ReadText()
        {
            var repo = RepoPath();
            if (repo != null)
                return File.ReadAllText(repo);
            var streaming = StreamingPath();
            if (File.Exists(streaming))
                return File.ReadAllText(streaming);
            throw new FileNotFoundException("interop/finnball.json not found in repo or StreamingAssets");
        }
    }
}
