import React from 'react';
import {useNavigate,} from 'react-router-dom';
import { withAuth0 } from '@auth0/auth0-react';
import BaseComponent from "../../components/BaseComponent/index.jsx";
import {Bloc} from "./bloc.js";
import {
  Box, Button
} from "@mui/material";


class Dashboard extends BaseComponent {

  constructor(props) {
    super(props);
    this.setBloc(new Bloc({auth0: props.auth0}));
  }


  render() {

    return <Box>
      <Box sx={{ padding: "16px" }}>
        Some dashboard <Button>test</Button>
      </Box>
    </Box>;
  }
}

export default withAuth0(Dashboard);