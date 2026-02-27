import { updateMediaAssetMetadata, type MediaAsset } from '$lib/features/media/api';
import { composeAssetName, parseTagList, splitNameAndExtension } from '$lib/features/media/utils';

export type MediaMetadataFields = {
  renameBase: string;
  description: string;
  tags: string;
};

export function hydrateMediaMetadata(asset: MediaAsset | null): MediaMetadataFields {
  if (!asset) {
    return { renameBase: '', description: '', tags: '' };
  }
  const { base } = splitNameAndExtension(asset.name);
  return {
    renameBase: base,
    description: asset.description ?? '',
    tags: asset.tags.join(', ')
  };
}

export async function saveMediaMetadata(asset: MediaAsset, fields: MediaMetadataFields): Promise<MediaAsset> {
  const nextName = composeAssetName(asset, fields.renameBase);
  return updateMediaAssetMetadata(asset.id, {
    name: nextName ?? undefined,
    description: fields.description.trim() || undefined,
    tags: parseTagList(fields.tags)
  });
}
