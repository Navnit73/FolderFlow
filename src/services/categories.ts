import {
  commands,
  type CategoryInput_Deserialize,
  type Category_Serialize,
} from "@/bindings";
import { unwrap } from "./result";

export type Category = Category_Serialize;
export type CategoryInput = CategoryInput_Deserialize;

export const categoriesService = {
  list: (): Promise<Category[]> => unwrap(commands.categoriesList()),
  create: (input: CategoryInput): Promise<Category> => unwrap(commands.categoriesCreate(input)),
  update: (id: string, input: CategoryInput): Promise<Category> => unwrap(commands.categoriesUpdate(id, input)),
  remove: (id: string): Promise<null> => unwrap(commands.categoriesDelete(id)),
  setEnabled: (id: string, enabled: boolean): Promise<Category> => unwrap(commands.categoriesSetEnabled(id, enabled)),
  reorder: (orderedIds: string[]): Promise<Category[]> => unwrap(commands.categoriesReorder(orderedIds)),
  pickDestinationFolder: (): Promise<string | null> => unwrap(commands.categoriesPickDestinationFolder()),
};

