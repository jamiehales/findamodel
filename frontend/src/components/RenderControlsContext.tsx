import React, { createContext, useContext, useMemo, useState } from 'react';

interface RenderControlsContextValue {
  showSupports: boolean;
  setShowSupports: React.Dispatch<React.SetStateAction<boolean>>;
  supportsToggleAvailable: boolean;
  setSupportsToggleAvailable: React.Dispatch<React.SetStateAction<boolean>>;
}

const RenderControlsContext = createContext<RenderControlsContextValue | null>(null);

export function RenderControlsProvider({ children }: React.PropsWithChildren) {
  const [showSupports, setShowSupports] = useState(true);
  const [supportsToggleAvailable, setSupportsToggleAvailable] = useState(false);

  const value = useMemo(
    () => ({
      showSupports,
      setShowSupports,
      supportsToggleAvailable,
      setSupportsToggleAvailable,
    }),
    [showSupports, supportsToggleAvailable],
  );

  return <RenderControlsContext.Provider value={value}>{children}</RenderControlsContext.Provider>;
}

export function useRenderControls() {
  const value = useContext(RenderControlsContext);
  if (value == null)
    throw new Error('useRenderControls must be used within a RenderControlsProvider');

  return value;
}
