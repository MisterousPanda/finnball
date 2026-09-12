using UnityEngine;

namespace Finnball
{
    /// <summary>Mirrors src/skills.rs. Ratings stay the gym baseline; these fire in flavour situations.</summary>
    public static class FinnballSkills
    {
        public const string BlinkDrive = "blink_drive";
        public const string LogoGravity = "logo_gravity";
        public const string FloorRemembers = "floor_remembers";
        public const string LockedGate = "locked_gate";
        public const string WatchTheIdea = "watch_the_idea";
        public const string NeverThere = "never_there";
        public const string ClutchRitual = "clutch_ritual";
        public const string MissBelongs = "miss_belongs";
        public const string ImpossibleEasy = "impossible_easy";
        public const string OnTheRise = "on_the_rise";

        public static float MakeMult(string id, SkillCtx ctx)
        {
            switch (id)
            {
                case BlinkDrive when ctx.Transition: return 1.10f;
                case LogoGravity when ctx.isThree && ctx.dist > 10f: return 1.22f;
                case LogoGravity when ctx.isThree && ctx.dist >= FinnballSim.ThreeRadius: return 1.16f;
                case FloorRemembers when ctx.inPaint && IsPaintFinish(ctx.shot): return 1.18f;
                case ClutchRitual when ctx.shotClock < 8f: return 1.14f;
                case ImpossibleEasy when ctx.contest > 0.30f && !ctx.isThree && IsFloaterFamily(ctx.shot):
                    return 1.15f;
                case OnTheRise when ctx.Transition && ctx.inPaint: return 1.16f;
                case MissBelongs when ctx.inPaint && ctx.dist < 2.4f: return 1.12f;
                default: return 1f;
            }
        }

        public static float StealMult(string id) => id == NeverThere ? 1.28f : 1f;
        public static float InterceptResist(string id) => id == WatchTheIdea ? 0.82f : 1f;
        public static float ContestMult(string id, bool defenderInPaint) =>
            id == LockedGate && defenderInPaint ? 1.35f : 1f;
        public static float ReboundMult(string id) => id == MissBelongs ? 1.25f : 1f;
        public static float SpeedMult(string id, bool sprint) =>
            id == BlinkDrive && sprint ? 1.08f : 1f;

        static bool IsPaintFinish(FinnballSim.ShotType shot) =>
            shot == FinnballSim.ShotType.Dunk || shot == FinnballSim.ShotType.Layup || shot == FinnballSim.ShotType.ReverseLayup;

        static bool IsFloaterFamily(FinnballSim.ShotType shot) =>
            shot == FinnballSim.ShotType.Floater || shot == FinnballSim.ShotType.Runner || shot == FinnballSim.ShotType.JumpShot;
    }

    public struct SkillCtx
    {
        public bool isThree;
        public float dist;
        public float contest;
        public float shotClock;
        public bool inPaint;
        public float speed;
        public FinnballSim.ShotType shot;

        public bool Transition => speed > 6f;
    }
}
