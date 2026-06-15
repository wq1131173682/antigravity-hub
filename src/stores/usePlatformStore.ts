import { create } from 'zustand';
import { Platform, ApiKey, Model, ModelUsageEntry } from '../types/platform';
import * as platformService from '../services/platformService';

interface PlatformState {
  platforms: Platform[];
  keys: Record<string, ApiKey[]>;
  models: Record<string, Model[]>;
  modelUsage: Record<string, ModelUsageEntry[]>;
  modelKeyIds: Record<string, string[]>;
  proxyRunning: boolean;
  proxyPort: number;
  loading: boolean;
  error: string | null;

  // Platform
  fetchPlatforms: () => Promise<void>;
  addPlatform: (name: string, baseUrl: string, pathPrefix: string) => Promise<void>;
  deletePlatform: (platformId: string) => Promise<void>;

  // Keys
  fetchKeys: (platformId: string) => Promise<ApiKey[]>;
  addKey: (platformId: string, name: string, keyValue: string) => Promise<void>;
  deleteKey: (platformId: string, keyId: string) => Promise<void>;
  setKeyStatus: (keyId: string, disabled: boolean, reason?: string) => Promise<void>;

  // Models
  fetchModels: (platformId: string) => Promise<Model[]>;
  addModel: (platformId: string, modelName: string, displayName: string, per5hour?: number, perDay?: number, perMonth?: number) => Promise<void>;
  updateModelLimits: (modelId: string, per5hour: number, perDay: number, perMonth: number) => Promise<void>;
  deleteModel: (platformId: string, modelId: string) => Promise<void>;

  // Model Usage
  fetchModelUsage: (modelId: string) => Promise<void>;

  // Key-Model Association
  getKeysForModel: (modelId: string) => Promise<string[]>;
  associateKeyWithModel: (keyId: string, modelId: string) => Promise<void>;
  disassociateKeyFromModel: (keyId: string, modelId: string) => Promise<void>;

  // Proxy
  fetchProxyStatus: () => Promise<void>;
  startProxy: () => Promise<void>;
  stopProxy: () => Promise<void>;
}

export const usePlatformStore = create<PlatformState>((set, get) => ({
  platforms: [],
  keys: {},
  models: {},
  modelUsage: {},
  modelKeyIds: {},
  proxyRunning: false,
  proxyPort: 8080,
  loading: false,
  error: null,

  // ========================================================================
  // Platform
  // ========================================================================

  fetchPlatforms: async () => {
    set({ loading: true, error: null });
    try {
      const platforms = await platformService.listPlatforms();
      set({ platforms, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  addPlatform: async (name: string, baseUrl: string, pathPrefix: string) => {
    set({ loading: true, error: null });
    try {
      await platformService.addPlatform(name, baseUrl, pathPrefix);
      await get().fetchPlatforms();
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  deletePlatform: async (platformId: string) => {
    set({ loading: true, error: null });
    try {
      await platformService.deletePlatform(platformId);
      const { keys, models } = get();
      const newKeys = { ...keys };
      const newModels = { ...models };
      delete newKeys[platformId];
      delete newModels[platformId];
      set({ keys: newKeys, models: newModels, loading: false });
      await get().fetchPlatforms();
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  // ========================================================================
  // Keys
  // ========================================================================

  fetchKeys: async (platformId: string) => {
    try {
      const keys = await platformService.listKeys(platformId);
      set(state => ({ keys: { ...state.keys, [platformId]: keys } }));
      return keys;
    } catch (e) {
      set({ error: String(e) });
      return [];
    }
  },

  addKey: async (platformId: string, name: string, keyValue: string) => {
    set({ loading: true, error: null });
    try {
      await platformService.addKey(platformId, name, keyValue);
      await get().fetchKeys(platformId);
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  deleteKey: async (platformId: string, keyId: string) => {
    try {
      await platformService.deleteKey(keyId);
      await get().fetchKeys(platformId);
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  setKeyStatus: async (keyId: string, disabled: boolean, reason?: string) => {
    try {
      if (disabled) {
        await platformService.disableKey(keyId, reason || 'Manually disabled');
      } else {
        await platformService.enableKey(keyId);
      }
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  // ========================================================================
  // Models
  // ========================================================================

  fetchModels: async (platformId: string) => {
    try {
      const models = await platformService.listModels(platformId);
      set(state => ({ models: { ...state.models, [platformId]: models } }));
      return models;
    } catch (e) {
      set({ error: String(e) });
      return [];
    }
  },

  addModel: async (platformId: string, modelName: string, displayName: string, per5hour?: number, perDay?: number, perMonth?: number) => {
    set({ loading: true, error: null });
    try {
      await platformService.addModel(platformId, modelName, displayName, per5hour, perDay, perMonth);
      await get().fetchModels(platformId);
      set({ loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  updateModelLimits: async (modelId: string, per5hour: number, perDay: number, perMonth: number) => {
    try {
      await platformService.updateModel(modelId, undefined, undefined, per5hour, perDay, perMonth);
      // Refresh all platforms' models to get updated state
      const { platforms, models } = get();
      for (const p of platforms) {
        if (models[p.id]) {
          await get().fetchModels(p.id);
          break;
        }
      }
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  deleteModel: async (platformId: string, modelId: string) => {
    try {
      await platformService.deleteModel(modelId);
      await get().fetchModels(platformId);
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  // ========================================================================
  // Model Usage
  // ========================================================================

  fetchModelUsage: async (modelId: string) => {
    try {
      const usage = await platformService.getModelUsage(modelId);
      set(state => ({ modelUsage: { ...state.modelUsage, [modelId]: usage } }));
    } catch (e) {
      console.error('Failed to fetch model usage:', e);
    }
  },

  // ========================================================================
  // Key-Model Association
  // ========================================================================

  getKeysForModel: async (modelId: string) => {
    try {
      const keyIds = await platformService.getKeysForModel(modelId);
      set(state => ({ modelKeyIds: { ...state.modelKeyIds, [modelId]: keyIds } }));
      return keyIds;
    } catch (e) {
      set({ error: String(e) });
      return [];
    }
  },

  associateKeyWithModel: async (keyId: string, modelId: string) => {
    try {
      await platformService.associateKeyWithModel(keyId, modelId);
      const keyIds = await platformService.getKeysForModel(modelId);
      set(state => ({ modelKeyIds: { ...state.modelKeyIds, [modelId]: keyIds } }));
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  disassociateKeyFromModel: async (keyId: string, modelId: string) => {
    try {
      await platformService.disassociateKeyFromModel(keyId, modelId);
      const keyIds = await platformService.getKeysForModel(modelId);
      set(state => ({ modelKeyIds: { ...state.modelKeyIds, [modelId]: keyIds } }));
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  // ========================================================================
  // Proxy
  // ========================================================================

  fetchProxyStatus: async () => {
    try {
      const status = await platformService.getProxyStatus();
      set({ proxyRunning: status.running, proxyPort: status.port });
    } catch (e) {
      console.error('Failed to fetch proxy status:', e);
    }
  },

  startProxy: async () => {
    try {
      // Port is read from Rust's PROXY_PORT static (set via set_proxy_port)
      await platformService.startProxy();
      set({ proxyRunning: true });
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  stopProxy: async () => {
    try {
      await platformService.stopProxy();
      set({ proxyRunning: false });
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },
}));
