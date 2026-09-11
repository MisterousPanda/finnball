using NUnit.Framework;
using UnityEngine;
using static Finnball.FinnballSim;

namespace Finnball.Tests
{
    /// <summary>Mirrors the #[test] block in src/sim.rs so the C# port cannot drift.</summary>
    public class FinnballSimTests
    {
        [Test]
        public void ThreeFromTheLogo()
        {
            Assert.AreEqual(ShotKind.Three, GetShotKind(0f, 0f, HoopX));
            Assert.AreEqual(3, PointsFor(ShotKind.Three));
        }

        [Test]
        public void LayupInPaintIsTwo()
        {
            Assert.AreEqual(ShotKind.Two, GetShotKind(HoopX - 1.2f, 0.2f, HoopX));
            Assert.IsTrue(InPaint(HoopX - 2f, 0.5f, HoopX));
        }

        [Test]
        public void ContestFallsOffWithDistance()
        {
            Assert.Greater(ContestFactor(0.4f, 90f), ContestFactor(2f, 90f));
            Assert.AreEqual(0f, ContestFactor(4f, 99f));
        }

        [Test]
        public void GreenMeterHelpsThrees()
        {
            var green = ShotMakeChance(92f, 7.2f, 0.05f, 0.02f, 0.9f, true);
            var brick = ShotMakeChance(92f, 7.2f, 0.05f, 0.8f, 0.9f, true);
            Assert.Greater(green, brick);
            Assert.Greater(green, 0.4f);
        }

        [Test]
        public void BallisticReachesTargetOnX()
        {
            var v = BallisticVelocity(new Vector3(0f, 2f, 0f), new Vector3(4f, 3.05f, 1f), 1f, Gravity);
            Assert.AreEqual(4f, v.x, 0.001f);
        }

        [Test]
        public void CourtClampKeepsPlayersInside()
        {
            var p = ClampToCourt(40f, -90f, 0.5f);
            Assert.IsTrue(InBounds(p.x, p.y, 0.5f));
        }

        [Test]
        public void ClassifyShotMatchesRust()
        {
            Assert.AreEqual(ShotType.Layup, ClassifyShot(2.5f, true, true, false, false, 50f, 70f, false, 4f));
            Assert.AreEqual(ShotType.Dunk, ClassifyShot(1.8f, true, true, false, false, 90f, 70f, true, 5f));
            Assert.AreEqual(ShotType.ThreePointer, ClassifyShot(7.1f, false, false, false, false, 40f, 80f, false, 1f));
        }

        [Test]
        public void DribbleSpeedsUpOnTheRun()
        {
            Assert.Greater(DribbleCadence(8f), DribbleCadence(1f));
        }

        [Test]
        public void AeroStepStaysFinite()
        {
            var spin = ReleaseSpin(ShotType.JumpShot, 1f, 1f, 0f);
            var v = ApplyAero(new Vector3(8f, 2f, 0f), spin, 0.016f);
            Assert.IsFalse(float.IsNaN(v.x) || float.IsInfinity(v.x));
            Assert.IsFalse(float.IsNaN(v.y) || float.IsInfinity(v.y));
            Assert.IsFalse(float.IsNaN(v.z) || float.IsInfinity(v.z));
        }

        [Test]
        public void GreenHomeJumperStillThreadsTheCylinder()
        {
            var from = new Vector3(0f, 1.95f, 0f);
            var hoop = HoopPosition(false);
            var t = FlightTimeForDistance(Mathf.Abs(hoop.x - from.x));
            var vel = BallisticVelocity(from, hoop, t, Gravity);
            var dt = 1f / 64f;
            var pos = from;
            var scored = false;
            for (var i = 0; i < 200; i++)
            {
                vel.y -= Gravity * dt;
                var prev = pos;
                pos += vel * dt;
                if (CylinderScore(prev, pos, vel.y, hoop))
                {
                    scored = true;
                    break;
                }
                if (pos.y < BallRadius && vel.y < 0f)
                    break;
            }
            Assert.IsTrue(scored, "green make on the solved ballistic must score");
        }

        [Test]
        public void OnFireBoostsMakes()
        {
            Assert.AreEqual(1f, HeatMakeMult(0));
            Assert.AreEqual(1f, HeatMakeMult(2));
            Assert.AreEqual(1.18f, HeatMakeMult(3), 1e-6f);
        }

        [Test]
        public void AiDoesNotHeaveFromLogoEarly()
        {
            Assert.IsFalse(AiWantsShot(12f, true, 90f, 18f, false));
            Assert.IsTrue(AiWantsShot(12f, true, 90f, 4f, false));
            Assert.IsTrue(AiWantsShot(3f, false, 50f, 20f, true));
            Assert.IsTrue(AiWantsShot(7f, true, 70f, 20f, false));
        }

        [Test]
        public void CylinderNeedsDownwardCross()
        {
            var hoop = HoopPosition(false);
            Assert.IsTrue(CylinderScore(hoop + Vector3.up * 0.1f, hoop - Vector3.up * 0.05f, -2f, hoop));
            Assert.IsFalse(CylinderScore(hoop - Vector3.up * 0.2f, hoop + Vector3.up * 0.1f, 2f, hoop));
        }

        [Test]
        public void TipoffSpotsAreInBounds()
        {
            foreach (var p in TipoffHome)
                Assert.IsTrue(InBounds(p.x, p.z, PlayerRadius));
            foreach (var p in TipoffAway)
                Assert.IsTrue(InBounds(p.x, p.z, PlayerRadius));
        }

        [Test]
        public void ManifestMatchesSimConstants()
        {
            var json = FinnballManifestLocator.ReadText();
            var data = FinnballManifest.FromJson(json);
            Assert.AreEqual(CourtHalfLen, data.court.half_length, 1e-5f);
            Assert.AreEqual(CourtHalfWid, data.court.half_width, 1e-5f);
            Assert.AreEqual(RimHeight, data.court.rim_height, 1e-5f);
            Assert.AreEqual(HoopX, data.court.hoop_x, 1e-5f);
            Assert.AreEqual(ThreeRadius, data.court.three_radius, 1e-5f);
            Assert.AreEqual(10, data.roster.Length);
            Assert.AreEqual(5, data.arenas.Length);
            for (var i = 0; i < 3; i++)
            {
                Assert.AreEqual(TipoffHome[i], data.spawns.home[i].ToUnity());
                Assert.AreEqual(TipoffAway[i], data.spawns.away[i].ToUnity());
            }
        }
    }
}
