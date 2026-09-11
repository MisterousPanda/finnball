using System;
using UnityEngine;

namespace Finnball
{
    /// <summary>
    /// Straight port of <c>src/sim.rs</c>: the pure basketball math the Bevy game runs on.
    /// Keep constants and formulas identical so a Unity build plays the same as the web build.
    /// Space is Y-up metres, X along the court (Bevy == Unity).
    /// </summary>
    public static class FinnballSim
    {
        public const float CourtHalfLen = 14.0f;
        public const float CourtHalfWid = 7.5f;
        public const float RimHeight = 3.05f;
        public const float RimRadius = 0.225f;
        public const float BackboardOffset = 0.42f;
        public const float HoopX = 12.55f;
        public const float BallRadius = 0.121f;
        public const float ThreeRadius = 6.75f;
        public const float PaintDepth = 5.8f;
        public const float PaintHalfWidth = 2.45f;
        public const float Gravity = 16.2f;
        public const float PlayerRadius = 0.38f;

        /// <summary>Fixed sim step the ball is integrated with (64 Hz).</summary>
        public const float SimDt = 1.0f / 64.0f;

        public const float StealReachFull = 0.55f;
        public const float StealReachMax = 1.25f;

        public static readonly Vector3[] TipoffHome =
        {
            new Vector3(-3.5f, 0f, 0f),
            new Vector3(-5.5f, 0f, 3.2f),
            new Vector3(-5.5f, 0f, -3.2f),
        };

        public static readonly Vector3[] TipoffAway =
        {
            new Vector3(3.5f, 0f, 0f),
            new Vector3(5.5f, 0f, 3.2f),
            new Vector3(5.5f, 0f, -3.2f),
        };

        public static readonly Vector3 TipoffBallHome = new Vector3(-3.1f, 1.1f, 0f);
        public static readonly Vector3 TipoffBallAway = new Vector3(3.1f, 1.1f, 0f);

        public enum ShotKind
        {
            Two,
            Three,
        }

        public enum ShotType
        {
            JumpShot,
            Fadeaway,
            Layup,
            ReverseLayup,
            FingerRoll,
            Dunk,
            Hook,
            Underhand,
            Floater,
            Runner,
            ThreePointer,
            LogoHeave,
        }

        public enum PassKind
        {
            Chest,
            Bounce,
            Lob,
            Skip,
        }

        public static Vector3 HoopPosition(bool homeHoop)
        {
            return new Vector3(homeHoop ? -HoopX : HoopX, RimHeight, 0f);
        }

        public static bool InBounds(float x, float z, float pad)
        {
            return Mathf.Abs(x) <= CourtHalfLen - pad && Mathf.Abs(z) <= CourtHalfWid - pad;
        }

        public static Vector2 ClampToCourt(float x, float z, float pad)
        {
            return new Vector2(
                Mathf.Clamp(x, -(CourtHalfLen - pad), CourtHalfLen - pad),
                Mathf.Clamp(z, -(CourtHalfWid - pad), CourtHalfWid - pad)
            );
        }

        public static float OffensiveHoopX(bool homeTeam)
        {
            return homeTeam ? HoopX : -HoopX;
        }

        public static bool InPaint(float x, float z, float hoopX)
        {
            var along = Mathf.Abs(x - hoopX);
            return along <= PaintDepth && Mathf.Abs(z) <= PaintHalfWidth;
        }

        public static ShotKind GetShotKind(float x, float z, float hoopX)
        {
            var dx = x - hoopX;
            var dist = Mathf.Sqrt(dx * dx + z * z);
            return dist >= ThreeRadius ? ShotKind.Three : ShotKind.Two;
        }

        public static int PointsFor(ShotKind kind)
        {
            return kind == ShotKind.Three ? 3 : 2;
        }

        /// <summary>
        /// Ballistic launch velocity to reach <paramref name="to"/> in <paramref name="flight"/> seconds.
        /// Includes the semi-implicit Euler drift compensation from sim.rs so the discrete
        /// trajectory lands exactly on the target.
        /// </summary>
        public static Vector3 BallisticVelocity(Vector3 from, Vector3 to, float flight, float gravity)
        {
            var t = Mathf.Max(flight, 0.18f);
            return new Vector3(
                (to.x - from.x) / t,
                (to.y - from.y + 0.5f * gravity * t * t) / t + 0.5f * gravity * SimDt,
                (to.z - from.z) / t
            );
        }

        /// <summary>0..1 make probability after contest, fatigue, meter, and ratings.</summary>
        public static float ShotMakeChance(
            float rating,
            float distance,
            float contest,
            float meterError,
            float stamina,
            bool isThree)
        {
            float rangeTerm = isThree
                ? Mathf.Clamp(1.15f - Mathf.Max(distance - ThreeRadius, 0f) * 0.09f, 0.25f, 1.15f)
                : Mathf.Clamp(1.2f - distance * 0.055f, 0.35f, 1.2f);
            var skill = Mathf.Clamp(rating / 100f, 0.2f, 1.15f);
            var open = Mathf.Clamp(1f - contest * 0.62f, 0.28f, 1f);
            var meter = Mathf.Clamp(1f - meterError * 1.35f, 0.15f, 1.08f);
            var gas = Mathf.Clamp(0.55f + stamina * 0.45f, 0.45f, 1f);
            return Mathf.Clamp(0.18f + 0.74f * skill * rangeTerm * open * meter * gas, 0.04f, 0.92f);
        }

        /// <summary>Consecutive makes: 3+ is ON FIRE.</summary>
        public static float HeatMakeMult(int streak)
        {
            return streak >= 3 ? 1.18f : 1f;
        }

        /// <summary>AI never heaves from the logo unless the shot clock is dying.</summary>
        public static bool AiWantsShot(float dist, bool open, float rating, float shotClock, bool closeRange)
        {
            if (dist >= 9.5f)
                return shotClock < 8f;
            return (open && rating > 62f) || shotClock < 5f || closeRange;
        }

        public static float StealChance(float stealRating, float handleRating, float distance)
        {
            var reach = Mathf.Clamp01(1f - (distance - StealReachFull) / (StealReachMax - StealReachFull));
            var mismatch = Mathf.Clamp((stealRating - handleRating) / 100f + 0.5f, 0.15f, 0.9f);
            return (0.12f + 0.45f * mismatch) * reach;
        }

        public static float ContestFactor(float defDist, float blockRating)
        {
            if (defDist > 2.4f)
                return 0f;
            var close = Mathf.Clamp01(1f - defDist / 2.4f);
            return close * (0.45f + blockRating / 180f);
        }

        public static float ContestWithHands(float defDist, float blockRating, bool handsUp)
        {
            var b = ContestFactor(defDist, blockRating);
            if (!handsUp)
                return b;
            return Mathf.Min(b * 1.45f + (defDist < 2.4f ? 0.08f : 0f), 1f);
        }

        public static float InterceptReach(float stealRating)
        {
            return Mathf.Clamp(0.40f + stealRating / 300f, 0.4f, 0.8f);
        }

        public static float ShotEv(float makeChance, bool isThree)
        {
            return makeChance * (isThree ? 3f : 2f);
        }

        public static float FlightTimeForDistance(float distance)
        {
            return Mathf.Clamp(0.72f + distance * 0.055f, 0.55f, 1.55f);
        }

        /// <summary>Rim catch: ball coming down, near rim centre, inside the cylinder.</summary>
        public static bool RimScoreWindow(Vector3 ball, float velY, Vector3 hoop)
        {
            if (velY > 0.4f)
                return false;
            var dx = ball.x - hoop.x;
            var dz = ball.z - hoop.z;
            var dy = ball.y - hoop.y;
            var r = RimRadius * 0.78f;
            return dx * dx + dz * dz <= r * r && Mathf.Abs(dy) < 0.16f;
        }

        public static string Label(ShotType shot)
        {
            switch (shot)
            {
                case ShotType.JumpShot: return "PULL-UP J";
                case ShotType.Fadeaway: return "FADEAWAY";
                case ShotType.Layup: return "LAYUP";
                case ShotType.ReverseLayup: return "REVERSE LAYUP";
                case ShotType.FingerRoll: return "FINGER ROLL";
                case ShotType.Dunk: return "POSTERIZE";
                case ShotType.Hook: return "HOOK SHOT";
                case ShotType.Underhand: return "UNDERHAND SCOOP";
                case ShotType.Floater: return "FLOATER";
                case ShotType.Runner: return "RUNNER";
                case ShotType.ThreePointer: return "FOR THREE";
                case ShotType.LogoHeave: return "FROM THE LOGO";
                default: throw new ArgumentOutOfRangeException(nameof(shot), shot, null);
            }
        }

        public static bool IsDunk(ShotType shot) => shot == ShotType.Dunk;

        /// <summary>Classify a release from court geometry + motion. Physics still decides the make.</summary>
        public static ShotType ClassifyShot(
            float dist,
            bool inThePaint,
            bool driving,
            bool movingAway,
            bool lateral,
            float dunkRating,
            float midRating,
            bool special,
            float speed)
        {
            if ((special || (inThePaint && driving && dunkRating > 58f)) && dist < PaintDepth + 0.8f)
                return ShotType.Dunk;
            if (dist > 10f)
                return ShotType.LogoHeave;
            if (dist >= ThreeRadius)
                return ShotType.ThreePointer;
            if (inThePaint && dist < 1.5f && speed < 2.2f)
                return ShotType.Underhand;
            if (inThePaint && driving && dist < 2.2f && midRating > 75f)
                return ShotType.FingerRoll;
            if (inThePaint && driving && dist < 2.8f && lateral)
                return ShotType.ReverseLayup;
            if (inThePaint && driving && dist < 3.5f)
                return ShotType.Layup;
            if (inThePaint && lateral && dist < 5f)
                return ShotType.Hook;
            if (!inThePaint && driving && dist < 6f)
                return ShotType.Floater;
            if (movingAway && dist < 8f)
                return ShotType.Fadeaway;
            if (speed > 5.5f && dist < 8f)
                return ShotType.Runner;
            return ShotType.JumpShot;
        }

        public static float ReleaseHeight(ShotType shot)
        {
            switch (shot)
            {
                case ShotType.Dunk: return 2.4f;
                case ShotType.Hook: return 2.35f;
                case ShotType.Underhand: return 1.05f;
                case ShotType.Layup:
                case ShotType.ReverseLayup: return 1.45f;
                case ShotType.FingerRoll: return 1.55f;
                case ShotType.Floater: return 1.75f;
                case ShotType.Runner: return 1.55f;
                case ShotType.Fadeaway: return 2.15f;
                default: return 1.95f;
            }
        }

        /// <summary>Backspin axis is shot_dir × up so a +X attack spins around −Z.</summary>
        public static Vector3 ReleaseSpin(ShotType shot, float quality, float dirX, float dirZ)
        {
            var q = Mathf.Clamp(quality, 0.5f, 1.1f);
            float mag;
            switch (shot)
            {
                case ShotType.Dunk: mag = 4f; break;
                case ShotType.Underhand: mag = 8f * q; break;
                case ShotType.Layup:
                case ShotType.ReverseLayup:
                case ShotType.FingerRoll: mag = 9f * q; break;
                case ShotType.Hook: mag = 10f * q; break;
                case ShotType.Floater:
                case ShotType.Runner: mag = 12f * q; break;
                default: mag = 18f * q; break;
            }
            var horiz = Mathf.Max(Mathf.Sqrt(dirX * dirX + dirZ * dirZ), 0.001f);
            var ax = dirZ / horiz;
            var az = -dirX / horiz;
            return new Vector3(ax * mag, 3f, az * mag);
        }

        public static float MeterAccuracy(float value)
        {
            return Mathf.Abs(value - 0.72f);
        }

        /// <summary>Dribble bounces per second — faster on the run so the ball stays glued.</summary>
        public static float DribbleCadence(float speedMps)
        {
            return 2.8f + speedMps * 0.35f;
        }

        /// <summary>Quadratic drag + Magnus (backspin lifts / sidespin curves).</summary>
        public static Vector3 ApplyAero(Vector3 vel, Vector3 spin, float dt)
        {
            const float airDensity = 1.2f;
            const float cd = 0.47f;
            const float mass = 0.62f;
            const float magnus = 0.012f;
            var area = Mathf.PI * BallRadius * BallRadius;
            var speed = vel.magnitude;
            var o = vel;
            if (speed > 0.01f)
            {
                var drag = 0.5f * airDensity * cd * area * speed;
                var inv = 1f / speed;
                o.x -= vel.x * inv * (drag / mass) * dt;
                o.y -= vel.y * inv * (drag / mass) * dt;
                o.z -= vel.z * inv * (drag / mass) * dt;
            }
            o.x += magnus * (spin.y * vel.z - spin.z * vel.y) / mass * dt;
            o.y += magnus * (spin.z * vel.x - spin.x * vel.z) / mass * dt;
            o.z += magnus * (spin.x * vel.y - spin.y * vel.x) / mass * dt;
            return o;
        }

        /// <summary>True if the ball crossed down through the rim cylinder this step.</summary>
        public static bool CylinderScore(Vector3 prev, Vector3 curr, float velY, Vector3 hoop)
        {
            if (velY > 0.15f)
                return false;
            var crossed = prev.y >= hoop.y && curr.y <= hoop.y + 0.08f;
            if (!crossed && !RimScoreWindow(curr, velY, hoop))
                return false;
            var dx = curr.x - hoop.x;
            var dz = curr.z - hoop.z;
            var r = RimRadius * 0.78f;
            return dx * dx + dz * dz <= r * r;
        }

        public static float SpeedFromRating(float rating, bool sprint)
        {
            var b = 4.2f + rating / 100f * 4.4f;
            return sprint ? b * 1.38f : b;
        }
    }
}
