#pragma once

#include "CoreMinimal.h"
#include "Kismet/BlueprintFunctionLibrary.h"
#include "FinnballTypes.h"
#include "FinnballLibrary.generated.h"

UCLASS()
class FINNBALL_API UFinnballLibrary : public UBlueprintFunctionLibrary
{
    GENERATED_BODY()

public:
    /** Convert a Bevy / Unity (Y-up metres) point into Unreal (Z-up cm). */
    UFUNCTION(BlueprintPure, Category = "FINNBALL")
    static FVector BevyToUnreal(float X, float Y, float Z)
    {
        return FFinnballVec3{X, Y, Z}.ToUnreal();
    }

    UFUNCTION(BlueprintPure, Category = "FINNBALL")
    static FFinnballCourt DefaultCourt()
    {
        return FFinnballCourt{};
    }
};
