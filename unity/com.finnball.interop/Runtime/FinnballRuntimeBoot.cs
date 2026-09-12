using System.IO;
using UnityEngine;
using static Finnball.FinnballSim;

namespace Finnball
{
    /// <summary>
    /// Playable Unity slice of the shipped game: WASD moves the controlled legend,
    /// Tab cycles the home lineup, Space / tap shoots with that legend's ratings and
    /// signature skill using the same ballistic + make-chance math as Bevy.
    /// </summary>
    public class FinnballRuntimeBoot : MonoBehaviour
    {
        FinnballManifest _data;
        Transform _ball;
        Transform[] _home = new Transform[3];
        int _ctrl;
        Vector3 _vel;
        Vector3 _spin;
        bool _inFlight;
        bool _held = true;
        float _acc;
        int _makes;
        int _attempts;
        string _lastShot = "ready";
        GUIStyle _style;
        GUIStyle _sub;
        Texture2D _portrait;

        static readonly int[] HomeLineup = { 0, 1, 2 };

        void Start()
        {
            _data = FinnballManifest.FromJson(FinnballManifestLocator.ReadText());
            _ball = transform.Find("Ball");
            for (var i = 0; i < 3; i++)
                _home[i] = transform.Find($"Player_home_{i}");
            LoadPortrait();
        }

        void Update()
        {
            if (_ball == null || _data == null)
                return;

            if (Input.GetKeyDown(KeyCode.Tab))
            {
                _ctrl = (_ctrl + 1) % 3;
                LoadPortrait();
            }

            if (!_inFlight)
                MoveController();

            var shoot = Input.GetKeyDown(KeyCode.Space)
                || (Input.touchCount > 0 && Input.GetTouch(0).phase == TouchPhase.Began);
            if (shoot && !_inFlight && _held)
                Shoot();

            if (!_inFlight)
                return;
            _acc += Time.deltaTime;
            var hoop = HoopPosition(false);
            while (_acc >= SimDt)
            {
                _acc -= SimDt;
                var prev = _ball.position;
                _vel.y -= Gravity * SimDt;
                var next = prev + _vel * SimDt;
                _ball.Rotate(_spin * (SimDt * Mathf.Rad2Deg), Space.World);
                _ball.position = next;
                if (CylinderScore(prev, next, _vel.y, hoop))
                {
                    _makes++;
                    _lastShot = "MAKE";
                    Rerack();
                    return;
                }
                if (next.y < BallRadius && _vel.y < 0f)
                {
                    _lastShot = "MISS";
                    Rerack();
                    return;
                }
            }
        }

        void MoveController()
        {
            var body = _home[_ctrl];
            if (body == null)
                return;
            var sprint = Input.GetKey(KeyCode.LeftShift) || Input.GetKey(KeyCode.RightShift);
            var legend = Legend();
            var skill = legend.skill != null ? legend.skill.id : "";
            var spd = SpeedFromRating(legend.ratings.speed, sprint)
                * FinnballSkills.SpeedMult(skill, sprint);
            var dir = new Vector3(Input.GetAxisRaw("Horizontal"), 0f, Input.GetAxisRaw("Vertical"));
            if (dir.sqrMagnitude > 0.04f)
            {
                dir.Normalize();
                var next = body.position + dir * (spd * Time.deltaTime);
            var pad = PlayerRadius;
            var clamped = ClampToCourt(next.x, next.z, pad);
                next.x = clamped.x;
                next.z = clamped.y;
                next.y = 0.94f;
                body.position = next;
                if (_held)
                {
                    var ball = next;
                    ball.y = 1.1f;
                    ball += dir * 0.45f;
                    _ball.position = ball;
                }
            }
        }

        void Shoot()
        {
            var legend = Legend();
            var from = _ball.position;
            var hoop = HoopPosition(false);
            var dist = Vector2.Distance(new Vector2(from.x, from.z), new Vector2(hoop.x, hoop.z));
            var inPaint = InPaint(from.x, from.z, hoop.x);
            var isThree = GetShotKind(from.x, from.z, hoop.x) == ShotKind.Three;
            var shot = ClassifyShot(
                dist, inPaint, false, false, false,
                legend.ratings.dunk, legend.ratings.mid, false, 0f);
            from.y = ReleaseHeight(shot);
            _ball.position = from;

            var rating = isThree ? legend.ratings.three : legend.ratings.mid;
            var ctx = new SkillCtx
            {
                isThree = isThree,
                dist = dist,
                contest = 0.08f,
                shotClock = 14f,
                inPaint = inPaint,
                speed = 0f,
                shot = shot,
            };
            var skillId = legend.skill != null ? legend.skill.id : "";
            var chance = Mathf.Min(
                ShotMakeChance(rating, dist, ctx.contest, 0.04f, 0.9f, isThree)
                    * FinnballSkills.MakeMult(skillId, ctx),
                0.96f);

            var target = hoop;
            if (Random.value > chance)
            {
                target += new Vector3(Random.Range(-0.55f, 0.55f), Random.Range(0.05f, 0.45f), Random.Range(-0.55f, 0.55f));
                _lastShot = $"{shot}  {Mathf.RoundToInt(chance * 100f)}%";
            }
            else
            {
                target += new Vector3(Random.Range(-0.04f, 0.04f), 0.02f, Random.Range(-0.04f, 0.04f));
                _lastShot = $"{shot}  {Mathf.RoundToInt(chance * 100f)}%";
            }

            var t = FlightTimeForDistance(dist);
            _vel = BallisticVelocity(from, target, t, Gravity);
            var dir = hoop - from;
            _spin = ReleaseSpin(shot, 1f, dir.x, dir.z);
            _inFlight = true;
            _held = false;
            _attempts++;
        }

        void Rerack()
        {
            _inFlight = false;
            _held = true;
            _vel = Vector3.zero;
            var body = _home[_ctrl];
            if (body != null)
                _ball.position = body.position + new Vector3(0.4f, 0.16f, 0f);
            else
                _ball.position = TipoffBallHome;
        }

        FinnballCharacter Legend() => _data.roster[HomeLineup[_ctrl]];

        void LoadPortrait()
        {
            _portrait = null;
            var legend = Legend();
            if (legend == null || string.IsNullOrEmpty(legend.portrait))
                return;
            var repo = FinnballManifestLocator.RepoPath();
            if (repo == null)
                return;
            var path = PathJoin(Directory.GetParent(Path.GetDirectoryName(repo)).FullName, "assets", legend.portrait);
            if (!File.Exists(path))
                path = Path.Combine(Application.streamingAssetsPath, legend.portrait);
            if (!File.Exists(path))
                return;
            var bytes = File.ReadAllBytes(path);
            var tex = new Texture2D(2, 2);
            if (ImageConversion.LoadImage(tex, bytes))
                _portrait = tex;
        }

        static string PathJoin(params string[] parts) => Path.Combine(parts);

        void OnGUI()
        {
            if (_style == null)
            {
                _style = new GUIStyle(GUI.skin.label) { fontSize = 20, fontStyle = FontStyle.Bold };
                _sub = new GUIStyle(GUI.skin.label) { fontSize = 14 };
            }
            var legend = _data != null ? Legend() : null;
            if (legend == null)
                return;
            if (_portrait != null)
                GUI.DrawTexture(new Rect(16, 12, 220, 124), _portrait, ScaleMode.ScaleToFit);
            var x = _portrait != null ? 250 : 16;
            var skill = legend.skill;
            GUI.Label(new Rect(x, 12, 720, 28), $"{legend.name}  ·  {legend.alias}", _style);
            GUI.Label(new Rect(x, 40, 720, 22), skill != null ? $"{skill.name}  —  {skill.hook}" : "", _sub);
            GUI.Label(
                new Rect(x, 64, 720, 40),
                $"SPD {legend.ratings.speed}  3PT {legend.ratings.three}  MID {legend.ratings.mid}  DUNK {legend.ratings.dunk}   {_makes}/{_attempts}  {_lastShot}\nWASD move  Shift sprint  Tab cycle  Space shoot",
                _sub);
        }
    }
}
