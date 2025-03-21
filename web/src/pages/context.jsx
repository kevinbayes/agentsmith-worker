import React, {ComponentType, createContext} from 'react';
import {useNavigate, useNavigationType} from "react-router-dom";

const appContext = createContext({});

export const AppContextProvider = appContext.Provider;
export const AppContextConsumer = appContext.Consumer;


export const withAppContext = (Component) => {
  return function withContext(props) {
    return (<AppContextConsumer>
      {value => {
        return (<Component {...props} globalContext={value} />);
        }
      }</AppContextConsumer>)
  };
};