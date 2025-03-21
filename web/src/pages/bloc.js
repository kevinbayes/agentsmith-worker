import {BaseBloc} from "../components/BaseComponent/bloc.js";
import {logger} from "../utils/logging.js";

export class Bloc extends BaseBloc {

  constructor(options) {
    super({ });
  }

  initialise = () => {

    logger.info(`initialise bloc.`);
    this.__makeInitialised({ });
  }

}

export class Event {

}
