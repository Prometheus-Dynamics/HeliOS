import type { ChannelPolicy } from '$lib/types/pipeline';

export const channelPolicyOptions: Array<{
  value: ChannelPolicy;
  label: string;
  description: string;
}> = [
  {
    value: 'NewestWins',
    label: 'Newest Wins',
    description: 'Drop the oldest buffered payload so downstream nodes receive the freshest data.'
  },
  {
    value: 'OldestWins',
    label: 'Preserve Order',
    description: 'Block upstream producers when buffers are full to guarantee lossless ordered delivery.'
  },
  {
    value: 'DropAll',
    label: 'Drop Newest',
    description: 'Discard new payloads immediately when buffers fill and let downstream nodes drain normally.'
  }
];
