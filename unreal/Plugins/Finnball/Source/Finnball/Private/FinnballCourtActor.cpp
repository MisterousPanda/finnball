#include "FinnballCourtActor.h"
#include "Components/StaticMeshComponent.h"
#include "UObject/ConstructorHelpers.h"

AFinnballCourtActor::AFinnballCourtActor()
{
    Root = CreateDefaultSubobject<USceneComponent>(TEXT("Root"));
    SetRootComponent(Root);

    Floor = CreateDefaultSubobject<UStaticMeshComponent>(TEXT("Floor"));
    Floor->SetupAttachment(Root);

    static ConstructorHelpers::FObjectFinder<UStaticMesh> Cube(TEXT("/Engine/BasicShapes/Cube"));
    if (Cube.Succeeded())
    {
        Floor->SetStaticMesh(Cube.Object);
    }

    const float Len = Court.HalfLength * 2.f * 100.f;
    const float Wid = Court.HalfWidth * 2.f * 100.f;
    Floor->SetRelativeScale3D(FVector(Len / 100.f, Wid / 100.f, 0.08f));
    Floor->SetRelativeLocation(FVector(0.f, 0.f, -4.f));
}
