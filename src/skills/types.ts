export type SkillFile = {
  path: string;
  content: string;
};

export type SkillDefinition = {
  id: string;
  type: 'skill';
  summary: string;
  instructions: string;
  source: string;
  files: SkillFile[];
};

export type SkillDiscovery = {
  getSkill: (id: string, path?: string) => Promise<SkillDefinition | undefined>;
};
