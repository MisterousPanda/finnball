#pragma once

#include "CoreMinimal.h"
#include "GameFramework/Actor.h"
#include "FinnballTypes.h"
#include "FinnballCourtActor.generated.h"

class UStaticMeshComponent;

/** Placeholder 3v3 court. Python `import_finnball.py` fills this from the JSON manifest. */
UCLASS()
class FINNBALL_API AFinnballCourtActor : public AActor
{
    GENERATED_BODY()

public:
    AFinnballCourtActor();

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    FFinnballCourt Court;

    UPROPERTY(EditAnywhere, BlueprintReadWrite, Category = "FINNBALL")
    FName ArenaId = TEXT("neo_tokyo");

    UPROPERTY(VisibleAnywhere, Category = "FINNBALL")
    TObjectPtr<USceneComponent> Root;

    UPROPERTY(VisibleAnywhere, Category = "FINNBALL")
    TObjectPtr<UStaticMeshComponent> Floor;
};
