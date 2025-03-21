import React from 'react';

import BaseComponent from "../BaseComponent/index.jsx";
import {Bloc} from "./bloc.js";
import {withAuth0} from "@auth0/auth0-react";
import {
    Box,
    CardContent,
    CardHeader,
    IconButton,
    List, ListItem, ListItemButton, ListItemIcon, ListItemText,
    Paper,
    Stack,
    styled, Tooltip,
    Typography,
} from "@mui/material";
import {withAppContext} from "../../pages/context.jsx";
import BlockOutlinedIcon from '@mui/icons-material/BlockOutlined';
import Groups2OutlinedIcon from '@mui/icons-material/Groups2Outlined';
import AccountTreeIcon from '@mui/icons-material/AccountTree';
import SportsEsportsIcon from '@mui/icons-material/SportsEsports';
import HubOutlinedIcon from '@mui/icons-material/HubOutlined';
import {withNavigation} from "../../utils/navigation.jsx";

const DrawerLink = function (props) {

    return <ListItem key={props.text} disablePadding sx={{ display: 'block', marginTop: "8px" }}>
        { props.open ? <ListItemButton onClick={props.onClick}>
                <ListItemIcon sx={{
                    minWidth: 0,
                    mr: 3,
                }}>
                    { props.icon }
                </ListItemIcon>
                <ListItemText primary={props.text}></ListItemText>
            </ListItemButton> :
            <Tooltip placement="right" title={props.text}>
                <ListItemButton onClick={props.onClick} sx={{
                    justifyContent: "center",
                }}>
                    <ListItemIcon sx={{
                        minWidth: 0,
                    }}>{ props.icon }</ListItemIcon>
                </ListItemButton>
            </Tooltip>
        }
    </ListItem>
}



class DrawerLeft extends BaseComponent {

    constructor(props) {
        super(props);
        this.setBloc(new Bloc({
            leftDrawer: 350,
        }, props.globalContext));

        this.state = {};
    }

    componentDidMount() {
        super.componentDidMount();
    }

    __navigate_to = (s) => (e) => {
        const { navigator } = this.props;
        navigator(s);
    }

    render() {

        const { initialised } = this.state;

        return (
            <Box sx={{padding: "60pt 0pt 7pt 0pt",}}>
                <List>
                    <DrawerLink key={'jobs'} icon={<HubOutlinedIcon />} text={'Jobs'} onClick={this.__navigate_to('/jobs')}></DrawerLink>
                </List>
            </Box>
        );
    }


}

export default withNavigation(withAuth0(withAppContext(DrawerLeft)));