// Perch's narrow DSH bridge: register the selected project through the upstream
// workspace API, preserving all existing sessions and settings.
export const name='perch-workspace';
export const inject=['workspaceRegistry','credentials'];
export async function apply(ctx){
 const credential=await ctx.credentials.resolve('PERCH_MODEL_KEY');
 if(!credential?.value || credential.value!==process.env.PERCH_MODEL_KEY)throw Error('Perch model connection was not resolved');
 await ctx.workspaceRegistry.create(process.cwd());
 console.log('PERCH_WORKSPACE_READY');
}
