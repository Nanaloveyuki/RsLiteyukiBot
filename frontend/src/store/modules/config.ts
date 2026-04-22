import { PayloadAction, createSlice } from '@reduxjs/toolkit';

import { createDefaultOneBotConfig, normalizeOneBotConfig } from '@/lib/onebot-config';

import type { RootState } from '@/store';

interface ConfigState {
  value: OneBotConfig;
}

const initialState: ConfigState = {
  value: createDefaultOneBotConfig(),
}

export const configSlice = createSlice({
  name: 'config',
  initialState,
  reducers: {
    updateConfig: (state, action: PayloadAction<OneBotConfig>) => {
      state.value = normalizeOneBotConfig(action.payload)
    },
  },
})

export const { updateConfig } = configSlice.actions;

export const selectCount = (state: RootState) => state.config.value;

export default configSlice.reducer;
