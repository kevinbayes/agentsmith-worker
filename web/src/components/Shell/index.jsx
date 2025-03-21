import React from 'react';

import MuiAppBar from '@mui/material/AppBar';

import BaseComponent from "../BaseComponent/index.jsx";
import {Bloc} from "./bloc.js";
import {useAuth0, withAuth0} from "@auth0/auth0-react";
import {
  Box, ButtonBase,
  Container, CssBaseline, Drawer,
  IconButton,
  Menu, MenuItem, styled,
  Toolbar,
  Typography
} from "@mui/material";
import {AccountCircle} from "@mui/icons-material";
import {withAppContext} from "../../pages/context.jsx";
import DrawerRight from "../DrawerRight/index.jsx";
import DrawerLeft from "../DrawerLeft/index.jsx";
import {withNavigation} from "../../utils/navigation.jsx";
import LocalNotifications from "../Notifications/index.jsx";

const LogoutMenuItem = () => {
    const { logout } = useAuth0();

    return (
        <MenuItem onClick={() => logout({ logoutParams: { returnTo: window.location.origin } })}>Logout</MenuItem>
    );
};

const ImageButton = styled(ButtonBase)(({ theme }) => ({
  '& img': {
    height: '100%',
    maxHeight: '64px',
  },
}));

const ShellContainer = styled(Container)(({ theme }) => ({
  paddingLeft: "0 !important",
  paddingRight: "0 !important",
}));

const ShellToolbar = styled(Toolbar)(({ theme }) => ({
  borderBottom: "1px solid #444444"
}));

const LogoButton = () => {

  return (
      <Box sx={{paddingLeft: '16px'}}>
          <p>Worker</p>
      </Box>
  );
};


class Shell extends BaseComponent {

    constructor(props) {
        super(props);
        this.setBloc(new Bloc({
            leftDrawer:72,
            rightDrawer: 72,
        }, props.globalContext));

        this.state = {};
    }

    componentDidMount() {
        super.componentDidMount();
    }

    __logout = (event) => {
        this.setState({anchorEl: event.currentTarget});
    };

    __showProfile = (event) => {
        this.setState({anchorEl: event.currentTarget});
    };

    __openAccount = (event) => {
        this.setState({anchorEl: event.currentTarget});
    };

     __handleMenu = (event) => {
        this.setState({anchorEl: event.currentTarget});
    };

    __handleMenuClose = () => {
        this.setState({anchorEl: null});
    };

    render() {

        const { anchorEl, leftDrawer, rightDrawer, } = this.state;

        return (
            <Box id={`shell`} sx={{ position: "relative", overflow: "hidden", height: "100dvh", minHeight: "100dvh", display: "flex", flexDirection: "column", }}>
                <MuiAppBar position="absolute" elevation={0} sx={{ zIndex: (theme) => theme.zIndex.drawer + 1, top: 0, }}>
                    <ShellContainer maxWidth={false}>
                        <ShellToolbar disableGutters>
                            <Box sx={{flexGrow: 1}}>
                              <LogoButton />
                            </Box>
                        </ShellToolbar>
                    </ShellContainer>
                </MuiAppBar>
                <Box sx={{ padding: "64px 72px 0 72px", display: "flex", flexDirection: "column", flex: "1 1 0%", overflow: "hidden", }}>
                  <Box sx={{ padding: "0", display: "flex", flexDirection: "column", flex: "1 1 0%", overflow: "auto",}}>
                      {this.props.children}
                  </Box>
                </Box>
                <Drawer
                    sx={{
                        width: leftDrawer,
                        flexShrink: 0,
                        '& .MuiDrawer-paper': {
                            width: leftDrawer,
                            boxSizing: 'border-box',
                        },
                    }}
                    variant="persistent"
                    anchor="left"
                    open={true}
                >
                  <DrawerLeft size={leftDrawer} />
                </Drawer>
                <Drawer
                    sx={{
                        width: rightDrawer,
                        flexShrink: 0,
                        '& .MuiDrawer-paper': {
                            width: rightDrawer,
                            boxSizing: 'border-box',
                            borderLeft: "1px solid #444444",
                        },
                    }}
                    variant="persistent"
                    anchor="right"
                    open={true}
                >
                    <DrawerRight size={rightDrawer} />
                </Drawer>
                <LocalNotifications />
            </Box>
        );
    }
}

export default withNavigation(withAuth0(withAppContext(Shell)));