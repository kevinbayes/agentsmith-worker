import React from 'react';
import BaseComponent from "../components/BaseComponent/index.jsx";
import {Bloc} from "./bloc.js";
import {Route, Routes,} from "react-router-dom";
import Shell from "../components/Shell/index.jsx";
import Dashboard from "./Dashboard/index.jsx";
import {AppContextProvider} from "./context.jsx";

class BaseRoute extends BaseComponent {

  constructor(props) {
    super(props);
    this.state = { };
    this.setBloc(new Bloc({ auth0: this.props.auth0 }));
  }

  componentDidMount() {
    super.componentDidMount();
    this.bloc.initialise();
  }

  render() {

    const { initialised, } = this.state;

    if(!initialised) {
      return <>Loading...</>
    }

    const context = {
      bloc: this.bloc,
    };

    return <AppContextProvider value={context}>
      <Shell>
        <Routes>
          <Route index element={ <Dashboard /> }></Route>
        </Routes>
      </Shell>
    </AppContextProvider> ;
  }
}

export default BaseRoute;