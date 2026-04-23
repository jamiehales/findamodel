import Slider from '@mui/material/Slider';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import type { AutoSupportSliceLayer } from '../lib/api';

interface AutoSupportLayerSliderProps {
  sliceLayers: AutoSupportSliceLayer[];
  selectedLayerIndex: number;
  onLayerChange: (layerIndex: number) => void;
  className?: string;
}

export default function AutoSupportLayerSlider({
  sliceLayers,
  selectedLayerIndex,
  onLayerChange,
  className,
}: AutoSupportLayerSliderProps) {
  const hasSliceLayers = sliceLayers.length > 0;
  const clampedLayerIndex = hasSliceLayers
    ? Math.min(Math.max(selectedLayerIndex, 0), sliceLayers.length - 1)
    : 0;

  return (
    <Stack className={className} spacing={1} alignItems="center">
      <Typography variant="caption" color="text.secondary">
        Top
      </Typography>
      <Slider
        orientation="vertical"
        min={0}
        max={Math.max(sliceLayers.length - 1, 0)}
        value={clampedLayerIndex}
        step={1}
        marks={false}
        disabled={!hasSliceLayers}
        onChange={(_, value) => {
          if (Array.isArray(value)) return;
          onLayerChange(value);
        }}
        valueLabelDisplay="auto"
        valueLabelFormat={(value) => {
          const layer = sliceLayers[value];
          return layer
            ? `L${layer.layerIndex + 1} ${layer.sliceHeightMm.toFixed(2)}mm`
            : `L${value + 1}`;
        }}
      />
      <Typography variant="caption" color="text.secondary">
        Base
      </Typography>
    </Stack>
  );
}
