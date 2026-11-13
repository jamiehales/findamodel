import { useCallback, useEffect, useRef } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { fetchSpinMesh } from '../../lib/api';
import { queryKeys } from '../../lib/queries';
import { spinPlayer } from './spinPlayer';

/**
 * Turns a model tile's spin mesh in 3D while a mouse hovers the tile. Put `imageRef` on the
 * tile's image box (it must be positioned; the canvas covers it) and the pointer handlers on
 * the tile. The mesh is fetched on the first hover and kept; without one (or on touch, or with
 * reduced motion) the tile keeps its PNG.
 *
 * `withSupports`: the tile shows the with-supports preview (the turning body never has
 * supports, but the first frame matches that preview's framing).
 */
export function useSpinPreview<T extends HTMLElement>(
  spinUrl: string | null,
  withSupports: boolean,
) {
  const queryClient = useQueryClient();
  const imageRef = useRef<T>(null);

  useEffect(() => {
    const image = imageRef.current;
    return () => {
      if (image) spinPlayer().stop(image);
    };
  }, []);

  const onPointerEnter = useCallback(
    (e: React.PointerEvent) => {
      const image = imageRef.current;
      if (!spinUrl || e.pointerType !== 'mouse' || !image) return;
      spinPlayer().start(image, withSupports, () =>
        queryClient.fetchQuery({
          queryKey: queryKeys.spinMesh(spinUrl),
          queryFn: () => fetchSpinMesh(spinUrl),
          staleTime: Infinity,
          gcTime: Infinity,
        }),
      );
    },
    [queryClient, spinUrl, withSupports],
  );

  const onPointerLeave = useCallback(() => {
    if (imageRef.current) spinPlayer().stop(imageRef.current);
  }, []);

  return { imageRef, onPointerEnter, onPointerLeave };
}
