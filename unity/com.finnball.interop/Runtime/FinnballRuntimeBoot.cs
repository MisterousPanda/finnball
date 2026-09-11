using UnityEngine;

namespace Finnball
{
    /// <summary>
    /// First playable slice in Unity: the ball uses the same ballistic solve and 64 Hz
    /// integration as the Bevy sim. Press Space (or tap) to shoot at the away hoop from
    /// wherever the ball is; it re-racks on a make or when it dies on the floor.
    /// </summary>
    public class FinnballRuntimeBoot : MonoBehaviour
    {
        Transform _ball;
        Vector3 _vel;
        Vector3 _spin;
        bool _inFlight;
        float _acc;
        int _makes;
        int _attempts;
        GUIStyle _style;

        void Start()
        {
            var t = transform.Find("Ball");
            _ball = t != null ? t : null;
            if (_ball == null)
                Debug.LogWarning("FINNBALL: no Ball child; runtime boot idle");
        }

        void Update()
        {
            if (_ball == null)
                return;
            var shoot = Input.GetKeyDown(KeyCode.Space) || (Input.touchCount > 0 && Input.GetTouch(0).phase == TouchPhase.Began);
            if (shoot && !_inFlight)
                Shoot();

            if (!_inFlight)
                return;
            _acc += Time.deltaTime;
            var hoop = FinnballSim.HoopPosition(false);
            while (_acc >= FinnballSim.SimDt)
            {
                _acc -= FinnballSim.SimDt;
                // Same as Bevy: a green release rides the solved ballistic (no aero) so it
                // threads the cylinder; aero only shapes misses/loose balls there.
                var prev = _ball.position;
                _vel.y -= FinnballSim.Gravity * FinnballSim.SimDt;
                var next = prev + _vel * FinnballSim.SimDt;
                _ball.Rotate(_spin * (FinnballSim.SimDt * Mathf.Rad2Deg), Space.World);
                _ball.position = next;
                if (FinnballSim.CylinderScore(prev, next, _vel.y, hoop))
                {
                    _makes++;
                    Rerack();
                    return;
                }
                if (next.y < FinnballSim.BallRadius && _vel.y < 0f)
                {
                    Rerack();
                    return;
                }
            }
        }

        void Shoot()
        {
            var from = _ball.position;
            var hoop = FinnballSim.HoopPosition(false);
            var dist = Vector2.Distance(new Vector2(from.x, from.z), new Vector2(hoop.x, hoop.z));
            var shot = FinnballSim.ClassifyShot(dist, FinnballSim.InPaint(from.x, from.z, hoop.x), false, false, false, 60f, 80f, false, 0f);
            from.y = FinnballSim.ReleaseHeight(shot);
            _ball.position = from;
            var t = FinnballSim.FlightTimeForDistance(dist);
            _vel = FinnballSim.BallisticVelocity(from, hoop, t, FinnballSim.Gravity);
            var dir = hoop - from;
            _spin = FinnballSim.ReleaseSpin(shot, 1f, dir.x, dir.z);
            _inFlight = true;
            _attempts++;
        }

        void Rerack()
        {
            _inFlight = false;
            _vel = Vector3.zero;
            _ball.position = FinnballSim.TipoffBallHome;
        }

        void OnGUI()
        {
            if (_style == null)
                _style = new GUIStyle(GUI.skin.label) { fontSize = 22, fontStyle = FontStyle.Bold };
            GUI.Label(new Rect(16, 12, 600, 40), $"FINNBALL  {_makes}/{_attempts}  — Space / tap to shoot", _style);
        }
    }
}
