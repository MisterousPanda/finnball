using System;
using UnityEngine;

namespace Finnball
{
    [Serializable]
    public class FinnballManifest
    {
        public int schema;
        public string name;
        public string description;
        public FinnballEngine engine;
        public FinnballCourt court;
        public FinnballHoop[] hoops;
        public FinnballSpawns spawns;
        public FinnballTeam[] teams;
        public FinnballCharacter[] roster;
        public FinnballArena[] arenas;

        public static FinnballManifest FromJson(string json)
        {
            var data = JsonUtility.FromJson<FinnballManifest>(json);
            if (data == null || data.court == null)
                throw new ArgumentException("FINNBALL manifest did not parse.");
            if (data.schema != 1)
                throw new ArgumentException($"Unsupported FINNBALL schema {data.schema}.");
            if (data.roster == null || data.roster.Length != 10)
                throw new ArgumentException("FINNBALL roster must list ten legends.");
            if (data.arenas == null || data.arenas.Length != 5)
                throw new ArgumentException("FINNBALL must list five arenas.");
            return data;
        }

        public FinnballArena Arena(string id)
        {
            foreach (var arena in arenas)
            {
                if (arena.id == id)
                    return arena;
            }
            throw new ArgumentException($"Unknown arena '{id}'.");
        }
    }

    [Serializable]
    public class FinnballEngine
    {
        public string bevy;
        public string up;
        public string units;
        public string forward;
    }

    [Serializable]
    public class FinnballCourt
    {
        public float half_length;
        public float half_width;
        public float rim_height;
        public float rim_radius;
        public float backboard_offset;
        public float hoop_x;
        public float ball_radius;
        public float three_radius;
        public float paint_depth;
        public float paint_half_width;
        public float player_radius;
    }

    [Serializable]
    public class FinnballHoop
    {
        public string id;
        public FinnballVec3 position;
        public float faces;
    }

    [Serializable]
    public class FinnballSpawns
    {
        public FinnballVec3[] home;
        public FinnballVec3[] away;
        public FinnballVec3 ball_home;
        public FinnballVec3 ball_away;
    }

    [Serializable]
    public class FinnballTeam
    {
        public string id;
        public string name;
        public string abbrev;
        public string primary;
        public string secondary;
    }

    [Serializable]
    public class FinnballCharacter
    {
        public string id;
        public string name;
        public string alias;
        public string quote;
        public string style;
        public float height_m;
        public FinnballRatings ratings;
        public FinnballLook look;
        public FinnballSkill skill;
        public string portrait;
        public FinnballKit kit;
    }

    [Serializable]
    public class FinnballSkill
    {
        public string id;
        public string name;
        public string hook;
    }

    [Serializable]
    public class FinnballRatings
    {
        public int speed;
        public int three;
        public int mid;
        public int dunk;
        public int handle;
        public int pass;
        public int steal;
        public int block;
        public int rebound;
        public int strength;
    }

    [Serializable]
    public class FinnballLook
    {
        public string hair;
        public string hair_color;
        public string skin;
        public string accent;
        public string eye;
    }

    [Serializable]
    public class FinnballKit
    {
        public string arm_sleeve;
        public string knee_sleeve;
        public string tattoo;
        public bool tights;
        public bool headband;
        public bool high_socks;
        public string shoe_primary;
        public string shoe_secondary;
        public int celebration;
    }

    [Serializable]
    public class FinnballArena
    {
        public string id;
        public string name;
        public string subtitle;
        public float bounce;
        public float hangtime;
        public string env_pano;
        public string plate;
        public FinnballPalette palette;
        public string[] banner_years;
        public FinnballRetired[] retired;
        public string[] ribbon_words;
    }

    [Serializable]
    public class FinnballPalette
    {
        public string floor_a;
        public string floor_b;
        public string accent;
        public string apron;
        public string line;
        public string paint;
        public string ambient;
        public string fog;
        public string crowd;
        public string roof;
        public string sky;
        public string suite_glow;
        public float[] emissive;
    }

    [Serializable]
    public class FinnballRetired
    {
        public string number;
        public string name;
    }

    [Serializable]
    public class FinnballVec3
    {
        public float x;
        public float y;
        public float z;

        public Vector3 ToUnity() => new Vector3(x, y, z);
    }

    public static class FinnballColor
    {
        public static Color Parse(string hex)
        {
            if (ColorUtility.TryParseHtmlString(hex, out var color))
                return color;
            throw new ArgumentException($"Bad FINNBALL color '{hex}'.");
        }
    }
}
