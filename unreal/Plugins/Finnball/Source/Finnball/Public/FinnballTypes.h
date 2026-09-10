#pragma once

#include "CoreMinimal.h"
#include "FinnballTypes.generated.h"

/** Bevy / Unity Y-up metres → Unreal Z-up centimetres. */
USTRUCT(BlueprintType)
struct FINNBALL_API FFinnballVec3
{
    GENERATED_BODY()

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float X = 0.f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float Y = 0.f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float Z = 0.f;

    FVector ToUnreal() const
    {
        return FVector(X * 100.f, Z * 100.f, Y * 100.f);
    }
};

USTRUCT(BlueprintType)
struct FINNBALL_API FFinnballCourt
{
    GENERATED_BODY()

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float HalfLength = 14.f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float HalfWidth = 7.5f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float RimHeight = 3.05f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float RimRadius = 0.225f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float BackboardOffset = 0.42f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float HoopX = 12.55f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float BallRadius = 0.121f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float ThreeRadius = 6.75f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float PaintDepth = 5.8f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float PaintHalfWidth = 2.45f;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    float PlayerRadius = 0.38f;
};
