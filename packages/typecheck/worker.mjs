import {parentPort,workerData} from "node:worker_threads";
import {checkMutants} from "./index.mjs";
try{parentPort.postMessage({result:await checkMutants(workerData.options,workerData.mutants)});}
catch(error){parentPort.postMessage({error:error.stack||String(error)});}
