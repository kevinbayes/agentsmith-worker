import {useNavigate, useNavigationType, useParams} from "react-router-dom";
import React from "react";

export const withNavigation = (WrappedComponent) => (props) => {
    const navigator = useNavigate();
    const navigation = useNavigationType();
    return <><WrappedComponent {...props} navigator={navigator} navigation={navigation} /></>;
};

export const withParams = (WrappedComponent) => (props) => {
    const pathParams = useParams();
    return <><WrappedComponent {...props} pathParams={pathParams} /></>;
};