import { invoke, call } from "./invoke";
import { commands } from "@/lib/bindings";
import type { OmoLocalFileData } from "@/types/omo";

export const omoApi = {
  readLocalFile: (): Promise<OmoLocalFileData> => invoke("read_omo_local_file"),
  getCurrentOmoProviderId: (): Promise<string> =>
    call(commands.getCurrentOmoProviderId()),
  disableCurrentOmo: (): Promise<void> => call(commands.disableCurrentOmo()),
};

export const omoSlimApi = {
  readLocalFile: (): Promise<OmoLocalFileData> =>
    invoke("read_omo_slim_local_file"),
  getCurrentProviderId: (): Promise<string> =>
    call(commands.getCurrentOmoSlimProviderId()),
  disableCurrent: (): Promise<void> => call(commands.disableCurrentOmoSlim()),
};
