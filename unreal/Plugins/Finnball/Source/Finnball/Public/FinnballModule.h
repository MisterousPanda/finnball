#pragma once

#include "Modules/ModuleManager.h"

class FFinnballModule : public IModuleInterface
{
public:
    virtual void StartupModule() override;
    virtual void ShutdownModule() override;
};
