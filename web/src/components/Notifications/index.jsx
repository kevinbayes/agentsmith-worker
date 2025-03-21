import React, {Component} from 'react';

import PropTypes from 'prop-types';
import IconButton from '@mui/material/IconButton';
import {Snackbar} from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";
import {notificationService} from "../../utils/notification.js";

class LocalNotifications extends Component {

    constructor(props) {
        super(props);

        this.state = {
        };

        this.handleNotification = this.handleNotification.bind(this);
        notificationService.registerStateCallback(this.handleNotification);
    }

    handleNotification = (notification) => {

        this.setState({
            ...notification,
        });
    };


    handleSuccessClose = (event, reason) => {

        this.setState({
            success: undefined,
        });
    };


    handleErrorClose = (event, reason) => {

        this.setState({
            error: undefined,
        });
    };



    render() {

        const { success, error } = this.state;

        let errorAlert = (<></>);

        if (error) {

            errorAlert = (
                <Snackbar
                    anchorOrigin={{
                        vertical: 'bottom',
                        horizontal: 'center',
                    }}
                    open={true}
                    autoHideDuration={6000}
                    message={<span id="message-id">{error}</span>}
                    onClose={this.handleErrorClose}
                    action={[
                        <IconButton
                            key="close"
                            aria-label="Close"
                            color="inherit"
                            onClick={this.handleErrorClose}
                        >
                            <CloseIcon/>
                        </IconButton>,
                    ]}
                />
            );
        }

        let successAlert = (<></>);

        if (success) {

            successAlert = (
                <Snackbar
                    anchorOrigin={{
                        vertical: 'bottom',
                        horizontal: 'center',
                    }}
                    open={true}
                    autoHideDuration={6000}
                    message={<span id="message-id">{success}</span>}
                    onClose={this.handleSuccessClose}
                    action={[
                        <IconButton
                            key="close"
                            aria-label="Close"
                            color="inherit"
                            onClick={this.handleSuccessClose}
                        >
                            <CloseIcon/>
                        </IconButton>,
                    ]}
                />
            );
        }


        return (
            <>
                {errorAlert}
                {successAlert}
            </>
        );
    }
}

export default LocalNotifications;
